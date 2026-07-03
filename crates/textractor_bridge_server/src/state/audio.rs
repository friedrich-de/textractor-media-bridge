use anyhow::{anyhow, Context, Result};
use bridge_protocol::{
    AssetKind, AudioEndReason, AudioFinishResponse, AudioState, AudioTrimInfoResponse,
    AudioTrimRequest, AudioTrimSource, LineId, LinePatch, LineRecord,
};
use std::time::Duration;
use tracing::debug;

use crate::{
    media::audio::{
        slice_pcm16_wav, FinishedMainAudio, FinishedTrimAudio, MAIN_AUDIO_MAX_DURATION_MS,
        TRIM_AUDIO_MAX_DURATION_MS, TRIM_AUDIO_POSTROLL_MS,
    },
    time::unix_ms_now,
};

use super::AppState;

const MIN_TRIM_DURATION_MS: u64 = 100;

impl AppState {
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

    pub async fn remove_audio(&self, line_id: LineId) -> Result<AudioFinishResponse> {
        self.inner.audio.remove_line_session(line_id);
        let audio = AudioState::NoAudio { reason: None };
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

    pub(super) fn spawn_audio_deadlines(&self, line_id: LineId, started_unix_ms: i64) {
        self.spawn_main_deadline(
            line_id,
            started_unix_ms.saturating_add(ms_to_i64(MAIN_AUDIO_MAX_DURATION_MS)),
        );
        self.spawn_trim_deadline(
            line_id,
            started_unix_ms.saturating_add(ms_to_i64(TRIM_AUDIO_MAX_DURATION_MS)),
        );
    }

    pub(super) async fn finish_recordings_for_new_line(&self, process_id: u32, end_unix_ms: i64) {
        for line_id in self
            .inner
            .audio
            .main_recording_line_ids_for_process(process_id)
        {
            if let Err(error) =
                self.finish_audio_at(line_id, AudioEndReason::LineAdvanced, end_unix_ms)
            {
                debug!(%error, line_id, process_id, "audio next-line finalize skipped");
            }
            self.spawn_trim_postroll_finish(
                line_id,
                end_unix_ms.saturating_add(ms_to_i64(TRIM_AUDIO_POSTROLL_MS)),
            );
        }
    }

    fn spawn_main_deadline(&self, line_id: LineId, deadline_unix_ms: i64) {
        let state = self.clone();
        tokio::spawn(async move {
            sleep_until_unix_ms(deadline_unix_ms).await;
            if state.inner.audio.is_main_recording(line_id) {
                if let Err(error) =
                    state.finish_audio_at(line_id, AudioEndReason::MaxDuration, deadline_unix_ms)
                {
                    debug!(%error, line_id, "audio max-duration finalize skipped");
                }
            }
        });
    }

    fn spawn_trim_deadline(&self, line_id: LineId, deadline_unix_ms: i64) {
        let state = self.clone();
        tokio::spawn(async move {
            sleep_until_unix_ms(deadline_unix_ms).await;
            if state.inner.audio.is_recording(line_id) {
                if let Err(error) = state.finish_trim_audio_at(
                    line_id,
                    AudioEndReason::MaxDuration,
                    deadline_unix_ms,
                ) {
                    debug!(%error, line_id, "trim audio max-duration finalize skipped");
                }
            }
        });
    }

    fn spawn_trim_postroll_finish(&self, line_id: LineId, deadline_unix_ms: i64) {
        let state = self.clone();
        tokio::spawn(async move {
            sleep_until_unix_ms(deadline_unix_ms).await;
            if state.inner.audio.is_recording(line_id) {
                if let Err(error) = state.finish_trim_audio_at(
                    line_id,
                    AudioEndReason::LineAdvanced,
                    deadline_unix_ms,
                ) {
                    debug!(%error, line_id, "trim post-roll finalize skipped");
                }
            }
        });
    }
}

async fn sleep_until_unix_ms(deadline_unix_ms: i64) {
    let now = unix_ms_now();
    let delay_ms = deadline_unix_ms.saturating_sub(now).max(0) as u64;
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

fn ms_to_i64(ms: u64) -> i64 {
    ms.min(i64::MAX as u64) as i64
}

pub(super) fn trim_info_for_line(line: &LineRecord) -> Result<AudioTrimInfoResponse> {
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
    asset: &bridge_protocol::AssetInfo,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::AppConfig, media::audio::encode_pcm16_wav};
    use bridge_protocol::{AssetInfo, LineSeq, PipeLineMeta};

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
        let source_bytes = encode_pcm16_wav(&source_samples, 48_000);
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
        let ready_bytes = encode_pcm16_wav(&source_samples[9_600..33_600], 48_000);
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
