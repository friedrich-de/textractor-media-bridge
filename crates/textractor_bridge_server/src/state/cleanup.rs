use anyhow::{anyhow, Result};
use bridge_protocol::{AssetInfo, AudioState, BrowserEvent, ErrorEvent, LineRecord};

use super::AppState;

impl AppState {
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

    pub async fn cleanup_assets_and_history(&self) -> Result<usize> {
        let removed = self.inner.assets.cleanup_expired()?;
        if removed.is_empty() {
            return Ok(0);
        }

        let mut purged = 0usize;
        for line in self.inner.history.all_lines() {
            if line_references_any_asset(&line, &removed)
                && self.inner.history.purge_line(line.line_id)?
            {
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
        Ok(purged)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::{
        AssetKind, AudioEndReason, AudioTrimSource, LineId, LineSeq, PipeLineMeta,
    };

    #[test]
    fn trim_source_assets_are_counted_as_line_assets() {
        let ready = audio_asset("asset_ready");
        let source = audio_asset("asset_source");
        let line = line(
            1,
            Some(AudioState::Ready {
                asset: ready,
                duration_ms: 500,
                end_reason: AudioEndReason::LineAdvanced,
                trim_source: Some(Box::new(AudioTrimSource {
                    asset: source,
                    source_duration_ms: 1_500,
                    start_ms: 400,
                    end_ms: 900,
                    can_extend: true,
                })),
                trim_recording_started_unix_ms: None,
            }),
        );

        assert!(line_references_any_asset(
            &line,
            &["asset_source".to_owned()]
        ));
    }

    fn line(line_seq: LineSeq, audio: Option<AudioState>) -> LineRecord {
        LineRecord {
            line_id: line_seq as LineId,
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
