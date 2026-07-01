use anyhow::{anyhow, Context, Result};
use bridge_protocol::{
    AssetInfo, AssetKind, AudioEndReason, AudioFinishResponse, AudioState, BrowserEvent,
    BrowserLineAddedEvent, ErrorEvent, LineId, LinePatch, LineRecord, LineSeq, LineUpdatedEvent,
    MinePrepareRequest, MinePrepareResponse, PipeLineEvent, RangeScreenshotPick, PROTOCOL_VERSION,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    assets::{resolve_ffmpeg_path, AssetStore},
    config::AppConfig,
    history::HistoryStore,
    media::{
        audio::{AudioManager, FinishedAudio},
        screenshot::ScreenshotManager,
        window::{resolve_process_window, resolve_process_window_title, NativeHwnd},
    },
};

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
    config: AppConfig,
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
    pub fn load(config: AppConfig) -> Result<Self> {
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
                config,
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

    pub fn config(&self) -> &AppConfig {
        &self.inner.config
    }

    pub fn dirs(&self) -> &AppDirs {
        &self.inner.dirs
    }

    pub fn pipe_name(&self) -> String {
        if self.inner.config.pipe.name == "auto" {
            bridge_protocol::default_pipe_name()
        } else {
            self.inner.config.pipe.name.clone()
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
            self.inner.audio.finish_line_session(line_id, reason),
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
                .finish_line_session_at(line_id, reason, end_unix_ms),
        )
    }

    fn finish_audio_with_result(
        &self,
        line_id: LineId,
        finished: Option<FinishedAudio>,
    ) -> Result<AudioFinishResponse> {
        if let Some(finished) = finished {
            let audio = match finished {
                FinishedAudio::Ready(captured) => {
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
                    }
                }
                FinishedAudio::NoAudio { reason } => AudioState::NoAudio {
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

    pub fn find_asset_info(&self, asset_id: &str) -> Option<AssetInfo> {
        self.inner.assets.find_asset_info(asset_id).or_else(|| {
            self.inner.history.all_lines().into_iter().find_map(|line| {
                line.screenshot
                    .into_iter()
                    .chain(line.audio.into_iter().filter_map(|audio| match audio {
                        AudioState::Ready { asset, .. } => Some(asset),
                        _ => None,
                    }))
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

        let mut lines = self.inner.history.get_lines_by_ids(&request.line_ids);
        if lines.len() != request.line_ids.len() {
            return Err(anyhow!("one or more selected lines were not found"));
        }
        lines.sort_by_key(|line| line.line_seq);

        let separator = request
            .range_sentence_separator
            .clone()
            .unwrap_or_else(|| self.inner.config.anki.range_sentence_separator.clone());
        let sentence = lines
            .iter()
            .map(|line| line.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(&separator);

        let pick = request
            .range_screenshot_pick
            .unwrap_or(self.inner.config.anki.range_screenshot_pick);
        let screenshot = pick_screenshot(&lines, pick)
            .map(|asset| {
                self.inner.assets.transcode_screenshot_to_jpeg(
                    asset,
                    self.inner.config.mining.screenshot_quality,
                )
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
                    break;
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

        if !self
            .inner
            .config
            .mining
            .audio_format
            .eq_ignore_ascii_case("mp3")
        {
            return Err(anyhow!(
                "unsupported mining audio format '{}'; only mp3 is currently supported",
                self.inner.config.mining.audio_format
            ));
        }

        let ffmpeg_path =
            resolve_ffmpeg_path(self.inner.config.mining.ffmpeg_path.as_deref()).ok_or_else(
                || {
                    anyhow!(
                        "ffmpeg.exe was not found; set mining.ffmpeg_path or place ffmpeg.exe next to the server"
                    )
                },
            )?;

        self.inner
            .assets
            .transcode_audio_to_mp3(
                audio_assets,
                &ffmpeg_path,
                self.inner.config.mining.audio_bitrate_kbps,
            )
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
    let screenshot_removed = line
        .screenshot
        .as_ref()
        .map(|asset| removed.iter().any(|id| id == &asset.asset_id))
        .unwrap_or(false);
    let audio_removed = match &line.audio {
        Some(AudioState::Ready { asset, .. }) => removed.iter().any(|id| id == &asset.asset_id),
        _ => false,
    };
    screenshot_removed || audio_removed
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
