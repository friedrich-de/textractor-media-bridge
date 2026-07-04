use anyhow::Result;
use bridge_protocol::{
    AudioState, BrowserEvent, BrowserLineAddedEvent, LineHistoryPage, LinePatch, LineRecord,
    LineSeq, PipeLineEvent, PipeLineMeta, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use tracing::warn;

use crate::media::window::resolve_process_window_title;

use super::AppState;

impl AppState {
    pub fn newest_seq(&self) -> Option<LineSeq> {
        self.inner.history.newest_seq()
    }

    pub fn line_page(
        &self,
        limit: usize,
        before_seq: Option<LineSeq>,
        after_seq: Option<LineSeq>,
        source_key: Option<&str>,
    ) -> LineHistoryPage {
        let mut page = self
            .inner
            .history
            .page(limit, before_seq, after_seq, source_key);
        self.enrich_window_titles(&mut page.lines);
        page
    }

    pub fn clear_lines(&self) -> Result<usize> {
        self.inner.audio.clear_sessions();
        Ok(self.inner.history.clear()?)
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

        if let Some(line) = self.try_join_progressive_line(&event)? {
            return Ok(Some(line));
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

        if let Some(AudioState::Recording { started_unix_ms }) = &line.audio {
            self.spawn_audio_deadlines(line_id, *started_unix_ms);
        }
        if self.inner.screenshots.enabled() {
            self.spawn_screenshot_capture(line_id, line.meta.process_id);
        }

        Ok(Some(line))
    }

    fn try_join_progressive_line(&self, event: &PipeLineEvent) -> Result<Option<LineRecord>> {
        if !self.config().lines.join_progressive_text {
            return Ok(None);
        }

        let Some(previous) = self.inner.history.newest_line() else {
            return Ok(None);
        };
        if !can_join_progressive_line(&previous, event) {
            return Ok(None);
        }

        let updated_text = event.text.clone();
        let updated = self.inner.history.update(previous.line_id, |line| {
            line.text = updated_text.clone();
        })?;
        let Some(line) = updated else {
            return Ok(None);
        };

        self.broadcast_line_update(
            line.line_seq,
            line.line_id,
            LinePatch {
                text: Some(line.text.clone()),
                ..LinePatch::default()
            },
        );
        if self.inner.screenshots.enabled() {
            self.spawn_screenshot_capture(line.line_id, line.meta.process_id);
        }

        Ok(Some(line))
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
}

fn can_join_progressive_line(previous: &LineRecord, event: &PipeLineEvent) -> bool {
    same_join_source(&previous.meta, &event.meta)
        && event.text.starts_with(&previous.text)
        && event.text.len() > previous.text.len()
        && !matches!(previous.audio.as_ref(), Some(AudioState::Ready { .. }))
}

fn same_join_source(previous: &PipeLineMeta, next: &PipeLineMeta) -> bool {
    previous.process_id == next.process_id
        && previous.thread_number == next.thread_number
        && previous.source == next.source
        && previous.arch == next.arch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::AppConfig, media::audio::AudioManager};
    use bridge_protocol::{AudioEndReason, LineId};

    #[tokio::test]
    async fn progressive_text_updates_existing_line_and_preserves_first_timestamp() {
        let (_tmp, state) = test_state(true, "off");

        let first = state
            .ingest_pipe_line(event(1, 10_000, "俺は", 7, 2))
            .await
            .unwrap()
            .unwrap();
        let joined = state
            .ingest_pipe_line(event(2, 11_000, "俺はすごい", 7, 2))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(joined.line_id, first.line_id);
        assert_eq!(joined.line_seq, first.line_seq);
        assert_eq!(joined.timestamp_unix_ms, 10_000);
        assert_eq!(joined.text, "俺はすごい");

        let lines = state.inner.history.all_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_id, first.line_id);
        assert_eq!(lines[0].timestamp_unix_ms, 10_000);
        assert_eq!(lines[0].text, "俺はすごい");
    }

    #[tokio::test]
    async fn progressive_text_keeps_first_audio_session_until_non_extension() {
        let (_tmp, state) = test_state(true, "auto");
        state
            .inner
            .audio
            .insert_test_samples(7, 0, samples_by_ms(20_000));

        let first = state
            .ingest_pipe_line(event(1, 10_000, "俺は", 7, 2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            main_recording_ids(&state.inner.audio, 7),
            vec![first.line_id]
        );

        let joined = state
            .ingest_pipe_line(event(2, 11_000, "俺はすごい", 7, 2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(joined.line_id, first.line_id);
        assert_eq!(
            main_recording_ids(&state.inner.audio, 7),
            vec![first.line_id]
        );
        assert_eq!(state.inner.history.all_lines().len(), 1);

        let next = state
            .ingest_pipe_line(event(3, 12_000, "次の行", 7, 2))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(next.line_id, first.line_id);

        let lines = state.inner.history.all_lines();
        assert_eq!(lines.len(), 2);
        let Some(AudioState::Ready {
            duration_ms,
            end_reason,
            ..
        }) = lines[0].audio.as_ref()
        else {
            panic!("joined line audio should be finalized by the next non-extension line");
        };
        assert_eq!(*duration_ms, 4_000);
        assert_eq!(*end_reason, AudioEndReason::LineAdvanced);
        assert!(matches!(lines[1].audio, Some(AudioState::Recording { .. })));
    }

    #[tokio::test]
    async fn progressive_text_does_not_join_different_thread() {
        let (_tmp, state) = test_state(true, "off");

        state
            .ingest_pipe_line(event(1, 10_000, "俺は", 7, 2))
            .await
            .unwrap();
        state
            .ingest_pipe_line(event(2, 11_000, "俺はすごい", 7, 3))
            .await
            .unwrap();

        assert_eq!(state.inner.history.all_lines().len(), 2);
    }

    #[tokio::test]
    async fn progressive_text_joining_can_be_disabled() {
        let (_tmp, state) = test_state(false, "off");

        state
            .ingest_pipe_line(event(1, 10_000, "俺は", 7, 2))
            .await
            .unwrap();
        state
            .ingest_pipe_line(event(2, 11_000, "俺はすごい", 7, 2))
            .await
            .unwrap();

        assert_eq!(state.inner.history.all_lines().len(), 2);
    }

    fn test_state(
        join_progressive_text: bool,
        audio_backend: &str,
    ) -> (tempfile::TempDir, AppState) {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.storage.data_dir = Some(tmp.path().to_path_buf());
        config.screenshot.backend = "off".to_owned();
        config.audio.backend = audio_backend.to_owned();
        config.lines.join_progressive_text = join_progressive_text;
        let state = AppState::load(config).unwrap();
        (tmp, state)
    }

    fn event(
        message_id: u64,
        timestamp_unix_ms: i64,
        text: &str,
        process_id: u32,
        thread_number: i64,
    ) -> PipeLineEvent {
        PipeLineEvent {
            event_type: "line".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            message_id,
            timestamp_unix_ms,
            text: text.to_owned(),
            meta: PipeLineMeta {
                process_id,
                thread_number,
                thread_name: None,
                window_title: None,
                is_current_select: true,
                arch: "x86".to_owned(),
                source: "textractor".to_owned(),
            },
        }
    }

    fn samples_by_ms(duration_ms: u64) -> Vec<i16> {
        vec![0; duration_ms as usize * 48]
    }

    fn main_recording_ids(manager: &AudioManager, process_id: u32) -> Vec<LineId> {
        manager.main_recording_line_ids_for_process(process_id)
    }
}
