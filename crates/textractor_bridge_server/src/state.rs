use anyhow::{anyhow, Context, Result};
use bridge_protocol::{
    AssetInfo, AssetKind, AudioEndReason, AudioFinishResponse, AudioState, AudioTrimInfoResponse,
    AudioTrimRequest, AudioTrimSource, BrowserEvent, BrowserLineAddedEvent, ErrorEvent, LineId,
    LinePatch, LineRecord, LineSeq, LineUpdatedEvent, MinePrepareRequest, MinePrepareResponse,
    PipeLineEvent, RangeScreenshotPick, PROTOCOL_VERSION,
};
use parking_lot::RwLock;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    assets::{resolve_ffmpeg_path, AssetStore},
    config::{AppConfig, AudioConfig},
    history::HistoryStore,
    media::{
        audio::{slice_pcm16_wav, AudioManager, FinishedMainAudio, FinishedTrimAudio},
        screenshot::ScreenshotManager,
        window::{resolve_process_window, resolve_process_window_title, NativeHwnd},
    },
};

const MIN_TRIM_DURATION_MS: u64 = 100;

#[derive(Debug, Clone)]
pub struct SseMessage {
    pub id: LineSeq,
    pub event_name: &'static str,
    pub payload: BrowserEvent,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    config: RwLock<AppConfig>,
    config_path: PathBuf,
    dirs: AppDirs,
    history: HistoryStore,
    assets: AssetStore,
    screenshots: ScreenshotManager,
    audio: AudioManager,
    events: broadcast::Sender<SseMessage>,
    session_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppDirs {
    pub root: PathBuf,
    pub history_path: PathBuf,
    pub assets_dir: PathBuf,
}

impl AppDirs {
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let root = config.storage.data_dir.clone().unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("data"))
                .join("TextractorMediaBridge")
        });
        let history_path = root.join("history.jsonl");
        let assets_dir = root.join("assets");
        std::fs::create_dir_all(&assets_dir)
            .with_context(|| format!("failed to create {}", assets_dir.display()))?;
        Ok(Self {
            root,
            history_path,
            assets_dir,
        })
    }
}

impl AppState {
    #[cfg(test)]
    pub fn load(config: AppConfig) -> Result<Self> {
        Self::load_with_config_path(config, None)
    }

    pub fn load_with_config_path(config: AppConfig, config_path: Option<PathBuf>) -> Result<Self> {
        let dirs = AppDirs::from_config(&config)?;
        let history = HistoryStore::load(dirs.history_path.clone())?;
        let assets = AssetStore::new(dirs.assets_dir.clone(), config.assets.clone())?;
        let screenshots = ScreenshotManager::new(config.screenshot.backend.clone());
        let audio = AudioManager::new(config.audio.clone());
        let (events, _) = broadcast::channel(512);
        let session_token = (config.server.lan_mode && config.server.session_token_required)
            .then(|| Uuid::new_v4().simple().to_string());

        Ok(Self {
            inner: Arc::new(AppStateInner {
                config: RwLock::new(config),
                config_path: config_path.unwrap_or_else(AppConfig::default_path),
                dirs,
                history,
                assets,
                screenshots,
                audio,
                events,
                session_token,
            }),
        })
    }

    pub fn config(&self) -> AppConfig {
        self.inner.config.read().clone()
    }

    pub fn update_audio_config(&self, audio: AudioConfig) -> Result<AppConfig> {
        let mut updated = self.config();
        updated.audio = audio.clone();
        updated.save_to_path(&self.inner.config_path)?;
        *self.inner.config.write() = updated.clone();
        self.inner.audio.update_config(audio);
        Ok(updated)
    }

    pub fn dirs(&self) -> &AppDirs {
        &self.inner.dirs
    }

    pub fn pipe_name(&self) -> String {
        let config = self.inner.config.read();
        if config.pipe.name == "auto" {
            bridge_protocol::default_pipe_name()
        } else {
            config.pipe.name.clone()
        }
    }

    pub fn session_token(&self) -> Option<&str> {
        self.inner.session_token.as_deref()
    }

    pub fn token_required(&self) -> bool {
        self.inner.session_token.is_some()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SseMessage> {
        self.inner.events.subscribe()
    }

    pub fn newest_seq(&self) -> Option<LineSeq> {
        self.inner.history.newest_seq()
    }

    pub fn line_page(
        &self,
        limit: usize,
        before_seq: Option<LineSeq>,
        after_seq: Option<LineSeq>,
        source_key: Option<&str>,
    ) -> bridge_protocol::LineHistoryPage {
        let mut page = self
            .inner
            .history
            .page(limit, before_seq, after_seq, source_key);
        self.enrich_window_titles(&mut page.lines);
        page
    }

    pub async fn ingest_pipe_line(&self, mut event: PipeLineEvent) -> Result<Option<LineRecord>> {
        if event.protocol_version != PROTOCOL_VERSION || event.event_type != "line" {
            warn!(
                protocol_version = event.protocol_version,
                event_type = event.event_type,
                "dropping incompatible pipe event"
            );
            return Ok(None);
        }

        self.finish_recordings_for_new_line(event.meta.process_id, event.timestamp_unix_ms)
            .await;
        if event.meta.window_title.is_none() {
            event.meta.window_title = resolve_process_window_title(event.meta.process_id);
        }

        let line_seq = self.inner.history.next_line_seq();
        let line_id = line_seq;
        let audio = self.inner.audio.start_line_session(
            line_id,
            event.meta.process_id,
            event.timestamp_unix_ms,
        );

        let line = LineRecord {
            line_id,
            line_seq,
            timestamp_unix_ms: event.timestamp_unix_ms,
            text: event.text,
            meta: event.meta,
            screenshot: None,
            audio,
            warnings: Vec::new(),
            ignored: false,
        };

        self.inner.history.upsert(line.clone())?;
        self.broadcast(
            line_seq,
            "line_added",
            BrowserEvent::LineAdded(BrowserLineAddedEvent { line: line.clone() }),
        );

        if matches!(line.audio, Some(AudioState::Recording { .. })) {
            self.spawn_audio_auto_finish(line_id);
        }
        if self.inner.screenshots.enabled() {
            self.spawn_screenshot_capture(line_id, line.meta.process_id);
        }

        Ok(Some(line))
    }

    pub async fn finish_audio(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
    ) -> Result<AudioFinishResponse> {
        self.finish_audio_with_result(
            line_id,
            self.inner.audio.finish_main_line_session(line_id, reason),
        )
    }

    fn finish_audio_at(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
        end_unix_ms: i64,
    ) -> Result<AudioFinishResponse> {
        self.finish_audio_with_result(
            line_id,
            self.inner
                .audio
                .finish_main_line_session_at(line_id, reason, end_unix_ms),
        )
    }

    fn finish_audio_with_result(
        &self,
        line_id: LineId,
        finished: Option<FinishedMainAudio>,
    ) -> Result<AudioFinishResponse> {
        if let Some(finished) = finished {
            let audio = match finished {
                FinishedMainAudio::Ready(captured) => {
                    let asset = self.inner.assets.store_bytes(
                        AssetKind::Audio,
                        captured.mime_type,
                        &captured.bytes,
                        None,
                        None,
                        Some(captured.duration_ms),
                    )?;
                    AudioState::Ready {
                        asset,
                        duration_ms: captured.duration_ms,
                        end_reason: captured.end_reason,
                        trim_source: None,
                        trim_recording_started_unix_ms: Some(
                            captured.trim_recording_started_unix_ms,
                        ),
                    }
                }
                FinishedMainAudio::NoAudio { reason } => AudioState::NoAudio {
                    reason: Some(reason),
                },
            };
            let updated = self.inner.history.update(line_id, |line| {
                line.audio = Some(audio.clone());
            })?;
            if let Some(line) = updated {
                self.broadcast_line_update(
                    line.line_seq,
                    line_id,
                    LinePatch {
                        audio: Some(line.audio.clone()),
                        ..LinePatch::default()
                    },
                );
            }
            return Ok(AudioFinishResponse {
                line_id,
                audio: Some(audio),
            });
        }

        let existing = self
            .inner
            .history
            .get_line(line_id)
            .ok_or_else(|| anyhow!("line not found"))?;
        Ok(AudioFinishResponse {
            line_id,
            audio: existing.audio,
        })
    }

    pub async fn finish_trim_audio(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
    ) -> Result<AudioFinishResponse> {
        if matches!(
            self.inner
                .history
                .get_line(line_id)
                .and_then(|line| line.audio),
            Some(AudioState::Recording { .. })
        ) {
            let _ = self.finish_audio(line_id, reason).await?;
        }
        self.finish_trim_audio_with_result(
            line_id,
            self.inner.audio.finish_trim_line_session(line_id, reason),
        )
    }

    fn finish_trim_audio_at(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
        end_unix_ms: i64,
    ) -> Result<AudioFinishResponse> {
        self.finish_trim_audio_with_result(
            line_id,
            self.inner
                .audio
                .finish_trim_line_session_at(line_id, reason, end_unix_ms),
        )
    }

    fn finish_trim_audio_with_result(
        &self,
        line_id: LineId,
        finished: Option<FinishedTrimAudio>,
    ) -> Result<AudioFinishResponse> {
        if let Some(finished) = finished {
            let updated_audio = match finished {
                FinishedTrimAudio::Ready(captured) => {
                    let source_asset = self.inner.assets.store_bytes(
                        AssetKind::Audio,
                        captured.mime_type,
                        &captured.source_bytes,
                        None,
                        None,
                        Some(captured.source_duration_ms),
                    )?;
                    self.inner.history.update(line_id, |line| {
                        if let Some(AudioState::Ready {
                            trim_source,
                            trim_recording_started_unix_ms,
                            ..
                        }) = line.audio.as_mut()
                        {
                            *trim_source = Some(AudioTrimSource {
                                asset: source_asset,
                                source_duration_ms: captured.source_duration_ms,
                                start_ms: captured.start_ms,
                                end_ms: captured.end_ms,
                                can_extend: true,
                            });
                            *trim_recording_started_unix_ms = None;
                        }
                    })?
                }
                FinishedTrimAudio::NoAudio { reason } => {
                    debug!(line_id, %reason, "trim audio finished without source");
                    self.inner.history.update(line_id, |line| {
                        if let Some(AudioState::Ready {
                            trim_recording_started_unix_ms,
                            ..
                        }) = line.audio.as_mut()
                        {
                            *trim_recording_started_unix_ms = None;
                        }
                    })?
                }
            };

            if let Some(line) = updated_audio {
                self.broadcast_line_update(
                    line.line_seq,
                    line_id,
                    LinePatch {
                        audio: Some(line.audio.clone()),
                        ..LinePatch::default()
                    },
                );
                return Ok(AudioFinishResponse {
                    line_id,
                    audio: line.audio,
                });
            }
        }

        let existing = self
            .inner
            .history
            .get_line(line_id)
            .ok_or_else(|| anyhow!("line not found"))?;
        Ok(AudioFinishResponse {
            line_id,
            audio: existing.audio,
        })
    }

    pub fn audio_trim_info(&self, line_id: LineId) -> Result<AudioTrimInfoResponse> {
        let line = self
            .inner
            .history
            .get_line(line_id)
            .ok_or_else(|| anyhow!("line not found"))?;
        trim_info_for_line(&line)
    }

    pub fn apply_audio_trim(
        &self,
        line_id: LineId,
        request: AudioTrimRequest,
    ) -> Result<AudioFinishResponse> {
        let info = self.audio_trim_info(line_id)?;
        validate_trim_request(&request, &info)?;

        let source_bytes = self
            .inner
            .assets
            .load_bytes(&info.source.asset_id)
            .with_context(|| format!("failed to load source audio {}", info.source.asset_id))?;
        let sliced = slice_pcm16_wav(&source_bytes, request.start_ms, request.end_ms)
            .map_err(|error| anyhow!("{error}"))?;

        let line = self
            .inner
            .history
            .get_line(line_id)
            .ok_or_else(|| anyhow!("line not found"))?;
        let end_reason = match line.audio.as_ref() {
            Some(AudioState::Ready { end_reason, .. }) => *end_reason,
            _ => return Err(anyhow!("line does not have ready audio")),
        };

        let asset = self.inner.assets.store_bytes(
            AssetKind::Audio,
            "audio/wav",
            &sliced.bytes,
            None,
            None,
            Some(sliced.duration_ms),
        )?;
        let audio = AudioState::Ready {
            asset,
            duration_ms: sliced.duration_ms,
            end_reason,
            trim_source: Some(AudioTrimSource {
                asset: info.source,
                source_duration_ms: info.source_duration_ms,
                start_ms: request.start_ms,
                end_ms: request.end_ms,
                can_extend: info.can_extend,
            }),
            trim_recording_started_unix_ms: None,
        };

        let updated = self.inner.history.update(line_id, |line| {
            line.audio = Some(audio.clone());
        })?;
        if let Some(line) = updated {
            self.broadcast_line_update(
                line.line_seq,
                line_id,
                LinePatch {
                    audio: Some(line.audio.clone()),
                    ..LinePatch::default()
                },
            );
        }

        Ok(AudioFinishResponse {
            line_id,
            audio: Some(audio),
        })
    }

    pub fn find_asset_info(&self, asset_id: &str) -> Option<AssetInfo> {
        self.inner.assets.find_asset_info(asset_id).or_else(|| {
            self.inner.history.all_lines().into_iter().find_map(|line| {
                line_assets(&line)
                    .into_iter()
                    .find(|asset| asset.asset_id == asset_id)
            })
        })
    }

    pub fn load_asset_bytes(&self, asset_id: &str) -> Result<Vec<u8>> {
        Ok(self.inner.assets.load_bytes(asset_id)?)
    }

    pub fn asset_base64(&self, asset_id: &str) -> Result<bridge_protocol::AssetBase64Response> {
        let asset = self
            .find_asset_info(asset_id)
            .ok_or_else(|| anyhow!("asset not found"))?;
        Ok(self.inner.assets.base64_response(&asset)?)
    }

    pub fn prepare_mine(&self, request: MinePrepareRequest) -> Result<MinePrepareResponse> {
        if request.line_ids.is_empty() {
            return Err(anyhow!("line_ids must not be empty"));
        }
        let config = self.config();

        let mut lines = self.inner.history.get_lines_by_ids(&request.line_ids);
        if lines.len() != request.line_ids.len() {
            return Err(anyhow!("one or more selected lines were not found"));
        }
        lines.sort_by_key(|line| line.line_seq);

        let separator = request
            .range_sentence_separator
            .clone()
            .unwrap_or_else(|| config.anki.range_sentence_separator.clone());
        let sentence = lines
            .iter()
            .map(|line| line.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(&separator);

        let pick = request
            .range_screenshot_pick
            .unwrap_or(config.anki.range_screenshot_pick);
        let screenshot = pick_screenshot(&lines, pick)
            .map(|asset| {
                self.inner
                    .assets
                    .transcode_screenshot_to_jpeg(asset, config.mining.screenshot_quality)
            })
            .transpose()?;

        let audio = self.prepare_audio_to_mp3(&ready_audio_assets(&lines))?;

        let first = lines.first().expect("non-empty lines");
        let last = lines.last().expect("non-empty lines");
        let source = if first.line_id == last.line_id {
            format!(
                "PID {} / {} / {}",
                first.meta.process_id,
                first
                    .meta
                    .thread_name
                    .as_deref()
                    .unwrap_or("unknown thread"),
                iso_timestamp(first.timestamp_unix_ms)
            )
        } else {
            format!(
                "PID {} / {} / {} - {}",
                first.meta.process_id,
                first
                    .meta
                    .thread_name
                    .as_deref()
                    .unwrap_or("unknown thread"),
                iso_timestamp(first.timestamp_unix_ms),
                iso_timestamp(last.timestamp_unix_ms)
            )
        };

        Ok(MinePrepareResponse {
            sentence,
            screenshot,
            audio,
            source,
            line_ids: lines.into_iter().map(|line| line.line_id).collect(),
        })
    }

    pub async fn cleanup_assets_and_history(&self) -> Result<usize> {
        let removed = self.inner.assets.cleanup_expired()?;
        if removed.is_empty() {
            return Ok(0);
        }

        let mut purged = 0usize;
        for line in self.inner.history.all_lines() {
            if line_references_any_asset(&line, &removed) {
                if self.inner.history.purge_line(line.line_id)? {
                    purged += 1;
                    self.broadcast(
                        line.line_seq,
                        "line_updated",
                        BrowserEvent::Error(ErrorEvent {
                            code: "line_purged".to_owned(),
                            message: format!("line {} purged with expired assets", line.line_id),
                        }),
                    );
                }
            }
        }
        Ok(purged)
    }

    fn spawn_audio_auto_finish(&self, line_id: LineId) {
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Some(reason) = state.inner.audio.line_end_reason(line_id) {
                    if let Err(error) = state.finish_audio(line_id, reason).await {
                        debug!(%error, line_id, "audio auto-finalize skipped");
                    }
                }
                if let Some(reason) = state.inner.audio.trim_line_end_reason(line_id) {
                    if let Err(error) = state.finish_trim_audio(line_id, reason).await {
                        debug!(%error, line_id, "trim audio auto-finalize skipped");
                    }
                }
                if !state.inner.audio.is_recording(line_id) {
                    break;
                }
            }
        });
    }

    fn prepare_audio_to_mp3(&self, audio_assets: &[AssetInfo]) -> Result<Option<AssetInfo>> {
        if audio_assets.is_empty() {
            return Ok(None);
        }

        let config = self.config();
        if !config.mining.audio_format.eq_ignore_ascii_case("mp3") {
            return Err(anyhow!(
                "unsupported mining audio format '{}'; only mp3 is currently supported",
                config.mining.audio_format
            ));
        }

        let ffmpeg_path = resolve_ffmpeg_path(config.mining.ffmpeg_path.as_deref()).ok_or_else(
            || {
                anyhow!(
                    "ffmpeg.exe was not found; set mining.ffmpeg_path or place ffmpeg.exe next to the server"
                )
            },
        )?;

        self.inner
            .assets
            .transcode_audio_to_mp3(audio_assets, &ffmpeg_path, config.mining.audio_bitrate_kbps)
            .with_context(|| {
                format!(
                    "failed to prepare MP3 audio using {}",
                    ffmpeg_path.display()
                )
            })
    }

    fn enrich_window_titles(&self, lines: &mut [LineRecord]) {
        let mut titles = HashMap::<u32, Option<String>>::new();
        for line in lines {
            if line.meta.window_title.is_some() {
                continue;
            }
            let title = titles
                .entry(line.meta.process_id)
                .or_insert_with(|| resolve_process_window_title(line.meta.process_id));
            line.meta.window_title.clone_from(title);
        }
    }

    async fn finish_recordings_for_new_line(&self, process_id: u32, end_unix_ms: i64) {
        for line_id in self.inner.audio.recording_line_ids_for_process(process_id) {
            if let Err(error) = self.finish_audio_at(line_id, AudioEndReason::Silence, end_unix_ms)
            {
                debug!(%error, line_id, process_id, "audio next-line finalize skipped");
            }
            if let Err(error) =
                self.finish_trim_audio_at(line_id, AudioEndReason::Silence, end_unix_ms)
            {
                debug!(%error, line_id, process_id, "trim audio next-line finalize skipped");
            }
        }
    }

    fn spawn_screenshot_capture(&self, line_id: LineId, process_id: u32) {
        let state = self.clone();
        tokio::spawn(async move {
            if let Err(error) = state.capture_screenshot_for_line(line_id, process_id).await {
                warn!(%error, line_id, process_id, "screenshot capture failed");
                let _ = state.add_warning(line_id, format!("screenshot capture failed: {error}"));
            }
        });
    }

    async fn capture_screenshot_for_line(&self, line_id: LineId, process_id: u32) -> Result<()> {
        let hwnd = resolve_process_window(process_id)
            .ok_or_else(|| anyhow!("no visible window found for process {process_id}"))?;
        let manager = self.inner.screenshots.clone();
        let captured = tokio::task::spawn_blocking(move || capture_with_manager(manager, hwnd))
            .await
            .context("screenshot worker panicked")??;
        debug!(
            line_id,
            process_id,
            backend = captured.backend,
            width = captured.width,
            height = captured.height,
            "screenshot captured"
        );

        let asset = self.inner.assets.store_bytes(
            AssetKind::Screenshot,
            "image/png",
            &captured.bytes,
            Some(captured.width),
            Some(captured.height),
            None,
        )?;

        let updated = self.inner.history.update(line_id, |line| {
            line.screenshot = Some(asset.clone());
        })?;
        if let Some(line) = updated {
            self.broadcast_line_update(
                line.line_seq,
                line_id,
                LinePatch {
                    screenshot: Some(line.screenshot.clone()),
                    warnings: Some(line.warnings.clone()),
                    ..LinePatch::default()
                },
            );
        }
        Ok(())
    }

    fn add_warning(&self, line_id: LineId, warning: String) -> Result<()> {
        let updated = self.inner.history.update(line_id, |line| {
            if !line.warnings.iter().any(|item| item == &warning) {
                line.warnings.push(warning.clone());
            }
        })?;
        if let Some(line) = updated {
            self.broadcast_line_update(
                line.line_seq,
                line_id,
                LinePatch {
                    warnings: Some(line.warnings.clone()),
                    ..LinePatch::default()
                },
            );
        }
        Ok(())
    }

    fn broadcast_line_update(&self, line_seq: LineSeq, line_id: LineId, patch: LinePatch) {
        self.broadcast(
            line_seq,
            "line_updated",
            BrowserEvent::LineUpdated(LineUpdatedEvent {
                line_id,
                line_seq,
                patch,
            }),
        );
    }

    fn broadcast(&self, id: LineSeq, event_name: &'static str, payload: BrowserEvent) {
        let _ = self.inner.events.send(SseMessage {
            id,
            event_name,
            payload,
        });
    }
}

fn capture_with_manager(
    manager: ScreenshotManager,
    hwnd: NativeHwnd,
) -> Result<crate::media::screenshot::CapturedScreenshot> {
    Ok(manager.capture_window(hwnd)?)
}

fn pick_screenshot(lines: &[LineRecord], pick: RangeScreenshotPick) -> Option<&AssetInfo> {
    match pick {
        RangeScreenshotPick::First => lines.iter().find_map(|line| line.screenshot.as_ref()),
        RangeScreenshotPick::Last => lines.iter().rev().find_map(|line| line.screenshot.as_ref()),
    }
}

fn ready_audio_assets(lines: &[LineRecord]) -> Vec<AssetInfo> {
    lines
        .iter()
        .filter_map(|line| match &line.audio {
            Some(AudioState::Ready { asset, .. }) => Some(asset.clone()),
            _ => None,
        })
        .collect()
}

fn line_references_any_asset(line: &LineRecord, removed: &[String]) -> bool {
    line_assets(line)
        .iter()
        .any(|asset| removed.iter().any(|id| id == &asset.asset_id))
}

fn line_assets(line: &LineRecord) -> Vec<AssetInfo> {
    let mut assets = Vec::new();
    if let Some(asset) = &line.screenshot {
        assets.push(asset.clone());
    }
    if let Some(audio) = &line.audio {
        assets.extend(audio_assets(audio.clone()));
    }
    assets
}

fn audio_assets(audio: AudioState) -> Vec<AssetInfo> {
    match audio {
        AudioState::Ready {
            asset, trim_source, ..
        } => {
            let mut assets = vec![asset];
            if let Some(trim_source) = trim_source {
                assets.push(trim_source.asset);
            }
            assets
        }
        AudioState::Recording { .. } | AudioState::NoAudio { .. } => Vec::new(),
    }
}

fn trim_info_for_line(line: &LineRecord) -> Result<AudioTrimInfoResponse> {
    match line.audio.as_ref() {
        Some(AudioState::Ready {
            asset,
            duration_ms,
            trim_source,
            ..
        }) => Ok(trim_info_from_ready(
            line.line_id,
            asset,
            *duration_ms,
            trim_source.as_ref(),
        )),
        Some(AudioState::Recording { .. }) => Err(anyhow!("audio is still recording")),
        Some(AudioState::NoAudio { .. }) | None => Err(anyhow!("line does not have ready audio")),
    }
}

fn trim_info_from_ready(
    line_id: LineId,
    asset: &AssetInfo,
    duration_ms: u64,
    trim_source: Option<&AudioTrimSource>,
) -> AudioTrimInfoResponse {
    if let Some(trim_source) = trim_source {
        return AudioTrimInfoResponse {
            line_id,
            source: trim_source.asset.clone(),
            source_duration_ms: trim_source.source_duration_ms,
            start_ms: trim_source.start_ms,
            end_ms: trim_source.end_ms,
            can_extend: trim_source.can_extend,
        };
    }

    let source_duration_ms = asset.duration_ms.unwrap_or(duration_ms);
    AudioTrimInfoResponse {
        line_id,
        source: asset.clone(),
        source_duration_ms,
        start_ms: 0,
        end_ms: source_duration_ms,
        can_extend: false,
    }
}

fn validate_trim_request(request: &AudioTrimRequest, info: &AudioTrimInfoResponse) -> Result<()> {
    if request.start_ms >= request.end_ms {
        return Err(anyhow!("trim start must be before trim end"));
    }
    if request.end_ms > info.source_duration_ms {
        return Err(anyhow!(
            "trim end must be within source audio duration of {}ms",
            info.source_duration_ms
        ));
    }
    if request.end_ms.saturating_sub(request.start_ms) < MIN_TRIM_DURATION_MS {
        return Err(anyhow!(
            "trimmed audio must be at least {MIN_TRIM_DURATION_MS}ms"
        ));
    }
    Ok(())
}

fn iso_timestamp(unix_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(unix_ms)
        .map(|date| date.to_rfc3339())
        .unwrap_or_else(|| unix_ms.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::PipeLineMeta;

    #[test]
    fn ready_audio_assets_collects_ready_audio_in_line_order() {
        let first = audio_asset("asset_first");
        let second = audio_asset("asset_second");
        let lines = vec![
            line(
                1,
                Some(AudioState::Ready {
                    asset: first,
                    duration_ms: 100,
                    end_reason: AudioEndReason::Silence,
                    trim_source: None,
                    trim_recording_started_unix_ms: None,
                }),
            ),
            line(
                2,
                Some(AudioState::Recording {
                    started_unix_ms: 1_000,
                }),
            ),
            line(3, Some(AudioState::NoAudio { reason: None })),
            line(
                4,
                Some(AudioState::Ready {
                    asset: second,
                    duration_ms: 120,
                    end_reason: AudioEndReason::Manual,
                    trim_source: None,
                    trim_recording_started_unix_ms: None,
                }),
            ),
            line(5, None),
        ];

        let asset_ids = ready_audio_assets(&lines)
            .into_iter()
            .map(|asset| asset.asset_id)
            .collect::<Vec<_>>();

        assert_eq!(asset_ids, vec!["asset_first", "asset_second"]);
    }

    #[test]
    fn trim_source_assets_are_counted_as_line_assets() {
        let ready = audio_asset("asset_ready");
        let source = audio_asset("asset_source");
        let line = line(
            1,
            Some(AudioState::Ready {
                asset: ready,
                duration_ms: 500,
                end_reason: AudioEndReason::Silence,
                trim_source: Some(AudioTrimSource {
                    asset: source,
                    source_duration_ms: 1_500,
                    start_ms: 400,
                    end_ms: 900,
                    can_extend: true,
                }),
                trim_recording_started_unix_ms: None,
            }),
        );

        assert!(line_references_any_asset(
            &line,
            &["asset_source".to_owned()]
        ));
    }

    #[test]
    fn ready_audio_without_trim_source_uses_ready_asset_as_full_source() {
        let mut asset = audio_asset("asset_ready");
        asset.duration_ms = Some(1_000);
        let line = line(
            1,
            Some(AudioState::Ready {
                asset,
                duration_ms: 1_000,
                end_reason: AudioEndReason::Manual,
                trim_source: None,
                trim_recording_started_unix_ms: None,
            }),
        );

        let info = trim_info_for_line(&line).expect("ready audio should expose trim info");
        assert_eq!(info.start_ms, 0);
        assert_eq!(info.end_ms, 1_000);
        assert!(!info.can_extend);
        validate_trim_request(
            &AudioTrimRequest {
                start_ms: 100,
                end_ms: 900,
            },
            &info,
        )
        .expect("trimming old audio should be valid");

        let shortened_info = AudioTrimInfoResponse {
            start_ms: 100,
            end_ms: 900,
            ..info
        };
        validate_trim_request(
            &AudioTrimRequest {
                start_ms: 0,
                end_ms: 900,
            },
            &shortened_info,
        )
        .expect("old audio can move anywhere within its source asset");
    }

    #[test]
    fn apply_audio_trim_slices_source_and_updates_ready_asset() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.storage.data_dir = Some(tmp.path().to_path_buf());
        let state = AppState::load(config).unwrap();

        let source_samples = vec![1_000i16; 48_000];
        let source_bytes = crate::media::audio::encode_pcm16_wav(&source_samples, 48_000);
        let source_asset = state
            .inner
            .assets
            .store_bytes(
                AssetKind::Audio,
                "audio/wav",
                &source_bytes,
                None,
                None,
                Some(1_000),
            )
            .unwrap();
        let ready_bytes =
            crate::media::audio::encode_pcm16_wav(&source_samples[9_600..33_600], 48_000);
        let ready_asset = state
            .inner
            .assets
            .store_bytes(
                AssetKind::Audio,
                "audio/wav",
                &ready_bytes,
                None,
                None,
                Some(500),
            )
            .unwrap();
        state
            .inner
            .history
            .upsert(line(
                1,
                Some(AudioState::Ready {
                    asset: ready_asset.clone(),
                    duration_ms: 500,
                    end_reason: AudioEndReason::Manual,
                    trim_source: Some(AudioTrimSource {
                        asset: source_asset.clone(),
                        source_duration_ms: 1_000,
                        start_ms: 200,
                        end_ms: 700,
                        can_extend: true,
                    }),
                    trim_recording_started_unix_ms: None,
                }),
            ))
            .unwrap();

        let response = state
            .apply_audio_trim(
                1,
                AudioTrimRequest {
                    start_ms: 100,
                    end_ms: 400,
                },
            )
            .unwrap();

        let Some(AudioState::Ready {
            asset,
            duration_ms,
            trim_source,
            end_reason,
            trim_recording_started_unix_ms: _,
        }) = response.audio
        else {
            panic!("trim should produce ready audio");
        };
        assert_ne!(asset.asset_id, ready_asset.asset_id);
        assert_eq!(duration_ms, 300);
        assert_eq!(asset.duration_ms, Some(300));
        assert_eq!(end_reason, AudioEndReason::Manual);
        let trim_source = trim_source.expect("trim source should be preserved");
        assert_eq!(trim_source.asset.asset_id, source_asset.asset_id);
        assert_eq!(trim_source.start_ms, 100);
        assert_eq!(trim_source.end_ms, 400);

        let bytes = state.load_asset_bytes(&asset.asset_id).unwrap();
        assert!(bytes.len() > 44);
    }

    fn line(line_seq: LineSeq, audio: Option<AudioState>) -> LineRecord {
        LineRecord {
            line_id: line_seq,
            line_seq,
            timestamp_unix_ms: 1_000 + line_seq as i64,
            text: format!("line {line_seq}"),
            meta: PipeLineMeta {
                process_id: 123,
                thread_number: 1,
                thread_name: Some("hook".to_owned()),
                window_title: Some("Game Window".to_owned()),
                is_current_select: true,
                arch: "x86".to_owned(),
                source: "test".to_owned(),
            },
            screenshot: None,
            audio,
            warnings: Vec::new(),
            ignored: false,
        }
    }

    fn audio_asset(asset_id: &str) -> AssetInfo {
        AssetInfo {
            asset_id: asset_id.to_owned(),
            kind: AssetKind::Audio,
            mime_type: "audio/wav".to_owned(),
            url: format!("/assets/{asset_id}"),
            width: None,
            height: None,
            duration_ms: Some(100),
            created_unix_ms: 0,
            byte_size: 1,
        }
    }
}
