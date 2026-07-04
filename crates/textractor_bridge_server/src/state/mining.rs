use anyhow::{anyhow, Context, Result};
use bridge_protocol::{
    AssetInfo, AudioState, LineRecord, MinePrepareRequest, MinePrepareResponse, RangeScreenshotPick,
};

use crate::assets::resolve_ffmpeg_path;

use super::AppState;

impl AppState {
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

        let source = mining_source(lines.first().expect("non-empty lines"));

        Ok(MinePrepareResponse {
            sentence,
            screenshot,
            audio,
            source,
            line_ids: lines.into_iter().map(|line| line.line_id).collect(),
        })
    }

    fn prepare_audio_to_mp3(&self, audio_assets: &[AssetInfo]) -> Result<Option<AssetInfo>> {
        if audio_assets.is_empty() {
            return Ok(None);
        }

        let config = self.config();
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

fn mining_source(line: &LineRecord) -> String {
    line.meta
        .window_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use bridge_protocol::{AssetKind, AudioEndReason, LineSeq, PipeLineMeta, RangeScreenshotPick};

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
                    end_reason: AudioEndReason::LineAdvanced,
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
    fn pick_screenshot_uses_requested_range_position() {
        let mut first = line(1, None);
        first.screenshot = Some(audio_asset("first"));
        let mut second = line(2, None);
        second.screenshot = Some(audio_asset("second"));
        let lines = vec![first, second];

        assert_eq!(
            pick_screenshot(&lines, RangeScreenshotPick::First)
                .map(|asset| asset.asset_id.as_str()),
            Some("first")
        );
        assert_eq!(
            pick_screenshot(&lines, RangeScreenshotPick::Last).map(|asset| asset.asset_id.as_str()),
            Some("second")
        );
    }

    #[test]
    fn prepare_mine_uses_window_title_as_source() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.storage.data_dir = Some(tmp.path().to_path_buf());
        let state = AppState::load(config).unwrap();

        state.inner.history.upsert(line(1, None)).unwrap();

        let response = state
            .prepare_mine(MinePrepareRequest {
                line_ids: vec![1],
                range_sentence_separator: None,
                range_screenshot_pick: None,
            })
            .unwrap();

        assert_eq!(response.source, "Game Window");
    }

    #[test]
    fn prepare_mine_uses_empty_source_when_window_title_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.storage.data_dir = Some(tmp.path().to_path_buf());
        let state = AppState::load(config).unwrap();

        let mut selected = line(1, None);
        selected.meta.window_title = None;
        state.inner.history.upsert(selected).unwrap();

        let response = state
            .prepare_mine(MinePrepareRequest {
                line_ids: vec![1],
                range_sentence_separator: None,
                range_screenshot_pick: None,
            })
            .unwrap();

        assert_eq!(response.source, "");
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
