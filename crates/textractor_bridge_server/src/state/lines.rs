use anyhow::Result;
use bridge_protocol::{
    AudioState, BrowserEvent, BrowserLineAddedEvent, LineHistoryPage, LineId, LinePatch,
    LineRecord, LineSeq, PipeLineEvent, PipeLineMeta, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use tracing::{debug, info, warn};

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
    ) -> LineHistoryPage {
        let mut page = self.inner.history.page(limit, before_seq, after_seq);
        self.enrich_window_titles(&mut page.lines);
        page
    }

    pub fn clear_lines(&self) -> Result<usize> {
        let event_id = self.inner.history.newest_seq().unwrap_or(0);
        self.inner.audio.clear_sessions();
        let cleared_lines = self.inner.history.clear()?;
        self.broadcast_lines_cleared(event_id, cleared_lines);
        Ok(cleared_lines)
    }

    pub fn delete_line(&self, line_id: LineId) -> Result<bool> {
        self.inner.audio.remove_line_session(line_id);
        let Some(line) = self.inner.history.get_line(line_id) else {
            return Ok(false);
        };

        let deleted = self.inner.history.purge_line(line_id)?;
        if deleted {
            self.broadcast_line_deleted(line.line_seq, line_id);
        }
        Ok(deleted)
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

        self.broadcast_websocket_text(event.text.clone());

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
            debug!(
                reason = "disabled",
                process_id = event.meta.process_id,
                thread_number = event.meta.thread_number,
                source = %event.meta.source,
                current_raw_len = event.text.len(),
                "progressive line join rejected"
            );
            return Ok(None);
        }

        let Some(previous) = self.inner.history.newest_line() else {
            return Ok(None);
        };
        let decision = match progressive_join_decision(&previous, event) {
            Ok(decision) => decision,
            Err(rejection) => {
                debug!(
                    reason = rejection.reason,
                    previous_line_id = previous.line_id,
                    previous_process_id = previous.meta.process_id,
                    previous_thread_number = previous.meta.thread_number,
                    previous_source = %previous.meta.source,
                    current_process_id = event.meta.process_id,
                    current_thread_number = event.meta.thread_number,
                    current_source = %event.meta.source,
                    previous_raw_len = rejection.previous_raw_len,
                    current_raw_len = rejection.current_raw_len,
                    previous_normalized_len = rejection.previous_normalized_len,
                    current_normalized_len = rejection.current_normalized_len,
                    "progressive line join rejected"
                );
                return Ok(None);
            }
        };

        let updated_text = event.text.clone();
        let updated = self.inner.history.update(previous.line_id, |line| {
            line.text = updated_text.clone();
        })?;
        let Some(line) = updated else {
            return Ok(None);
        };

        info!(
            previous_line_id = previous.line_id,
            current_message_id = event.message_id,
            process_id = event.meta.process_id,
            thread_number = event.meta.thread_number,
            source = %event.meta.source,
            previous_raw_len = decision.previous_raw_len,
            current_raw_len = decision.current_raw_len,
            previous_normalized_len = decision.previous_normalized_len,
            current_normalized_len = decision.current_normalized_len,
            "progressive line joined"
        );
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgressiveJoinMatch {
    previous_raw_len: usize,
    current_raw_len: usize,
    previous_normalized_len: usize,
    current_normalized_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgressiveJoinRejection {
    reason: &'static str,
    previous_raw_len: usize,
    current_raw_len: usize,
    previous_normalized_len: usize,
    current_normalized_len: usize,
}

fn progressive_join_decision(
    previous: &LineRecord,
    event: &PipeLineEvent,
) -> Result<ProgressiveJoinMatch, ProgressiveJoinRejection> {
    let previous_normalized = normalize_progressive_text(&previous.text);
    let current_normalized = normalize_progressive_text(&event.text);
    let stats = ProgressiveJoinMatch {
        previous_raw_len: previous.text.len(),
        current_raw_len: event.text.len(),
        previous_normalized_len: previous_normalized.len(),
        current_normalized_len: current_normalized.len(),
    };

    if !same_join_source(&previous.meta, &event.meta) {
        return Err(rejection("different_source", stats));
    }
    if matches!(previous.audio.as_ref(), Some(AudioState::Ready { .. })) {
        return Err(rejection("previous_audio_ready", stats));
    }
    if !current_normalized.starts_with(&previous_normalized)
        || current_normalized.len() <= previous_normalized.len()
    {
        return Err(rejection("not_extension", stats));
    }

    Ok(stats)
}

fn rejection(reason: &'static str, stats: ProgressiveJoinMatch) -> ProgressiveJoinRejection {
    ProgressiveJoinRejection {
        reason,
        previous_raw_len: stats.previous_raw_len,
        current_raw_len: stats.current_raw_len,
        previous_normalized_len: stats.previous_normalized_len,
        current_normalized_len: stats.current_normalized_len,
    }
}

fn normalize_progressive_text(text: &str) -> String {
    strip_rich_text_tags(text)
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .replace("\\r", "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end()
        .to_owned()
}

fn strip_rich_text_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            let mut probe = chars.clone();
            let mut found_end = false;
            for next in &mut probe {
                if next == '>' {
                    found_end = true;
                    break;
                }
                tag.push(next);
                if tag.len() > 96 {
                    break;
                }
            }

            if found_end && is_known_rich_text_tag(&tag) {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn is_known_rich_text_tag(tag: &str) -> bool {
    let tag = tag.trim();
    let tag = tag.strip_prefix('/').unwrap_or(tag);
    let name = tag
        .split(|ch: char| ch == '=' || ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(name.as_str(), "size" | "color" | "line-height")
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
    async fn accepted_line_events_broadcast_plain_websocket_text() {
        let (_tmp, state) = test_state(true, "off");
        let mut websocket_text = state.subscribe_websocket_text();

        state
            .ingest_pipe_line(event(1, 10_000, "raw textractor text", 7, 2))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(websocket_text.recv().await.unwrap(), "raw textractor text");
    }

    #[tokio::test]
    async fn progressive_updates_broadcast_each_raw_websocket_text() {
        let (_tmp, state) = test_state(true, "off");
        let mut websocket_text = state.subscribe_websocket_text();

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

        assert_eq!(first.line_id, joined.line_id);
        assert_eq!(state.inner.history.all_lines().len(), 1);
        assert_eq!(websocket_text.recv().await.unwrap(), "俺は");
        assert_eq!(websocket_text.recv().await.unwrap(), "俺はすごい");
    }

    #[tokio::test]
    async fn invalid_pipe_events_do_not_broadcast_websocket_text() {
        let (_tmp, state) = test_state(true, "off");
        let mut websocket_text = state.subscribe_websocket_text();
        let mut invalid = event(1, 10_000, "ignored", 7, 2);
        invalid.protocol_version = 0;

        assert!(state.ingest_pipe_line(invalid).await.unwrap().is_none());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), websocket_text.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn progressive_text_joins_when_current_rewrites_before_trailing_newlines() {
        let (_tmp, state) = test_state(true, "off");
        let first_text = r#"<size=+4><color=#956f6e>圭一</color>\n</size>「レナが来るまでずーっと待ってる。

"#;
        let second_text = r#"<size=+4><color=#956f6e>圭一</color>\n</size>「レナが来るまでずーっと待ってる。いつまでも。」

"#;

        let first = state
            .ingest_pipe_line(event(1, 10_000, first_text, 7, 2))
            .await
            .unwrap()
            .unwrap();
        let joined = state
            .ingest_pipe_line(event(2, 11_000, second_text, 7, 2))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(joined.line_id, first.line_id);
        assert_eq!(joined.text, second_text);
        assert_eq!(state.inner.history.all_lines().len(), 1);
    }

    #[test]
    fn progressive_text_comparison_ignores_known_tags_and_trailing_layout_whitespace() {
        let previous = line_record(
            1,
            r#"<line-height=+6><size=+4><color=#956f6e>圭一</color>\n</size></line-height>「レナが来るまでずーっと待ってる。

"#,
            7,
            2,
            Some(AudioState::Recording {
                started_unix_ms: 10_000,
            }),
        );
        let current = event(
            2,
            11_000,
            r#"<size=+4><color=#956f6e>圭一</color>\n</size>「レナが来るまでずーっと待ってる。いつまでも。」

"#,
            7,
            2,
        );

        let decision = progressive_join_decision(&previous, &current);

        assert!(decision.is_ok());
    }

    #[test]
    fn progressive_text_comparison_rejects_equal_and_shortened_text() {
        let previous = line_record(
            1,
            "<size=+4>\\n</size>same text\n\n",
            7,
            2,
            Some(AudioState::Recording {
                started_unix_ms: 10_000,
            }),
        );
        let equal = event(2, 11_000, "<size=+4>\\n</size>same text", 7, 2);
        let shortened = event(3, 12_000, "<size=+4>\\n</size>same", 7, 2);

        assert_eq!(
            progressive_join_decision(&previous, &equal)
                .unwrap_err()
                .reason,
            "not_extension"
        );
        assert_eq!(
            progressive_join_decision(&previous, &shortened)
                .unwrap_err()
                .reason,
            "not_extension"
        );
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

    #[tokio::test]
    async fn clear_lines_clears_history_and_broadcasts_event() {
        let (_tmp, state) = test_state(true, "off");
        let mut events = state.subscribe();

        state
            .ingest_pipe_line(event(1, 10_000, "first", 7, 2))
            .await
            .unwrap();
        state
            .ingest_pipe_line(event(2, 11_000, "second", 7, 2))
            .await
            .unwrap();
        while events.try_recv().is_ok() {}

        let cleared = state.clear_lines().unwrap();

        assert_eq!(cleared, 2);
        assert!(state.inner.history.all_lines().is_empty());
        let event = events.recv().await.unwrap();
        assert_eq!(event.event_name, "lines_cleared");
        assert_eq!(event.id, 2);
        assert!(matches!(
            event.payload,
            BrowserEvent::LinesCleared(bridge_protocol::LinesClearedEvent { cleared_lines: 2 })
        ));
    }

    #[tokio::test]
    async fn delete_line_removes_one_line_and_broadcasts_event() {
        let (_tmp, state) = test_state(true, "off");
        let mut events = state.subscribe();

        let first = state
            .ingest_pipe_line(event(1, 10_000, "first", 7, 2))
            .await
            .unwrap()
            .unwrap();
        let second = state
            .ingest_pipe_line(event(2, 11_000, "second", 7, 2))
            .await
            .unwrap()
            .unwrap();
        while events.try_recv().is_ok() {}

        let deleted = state.delete_line(first.line_id).unwrap();

        assert!(deleted);
        let lines = state.inner.history.all_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_id, second.line_id);
        let event = events.recv().await.unwrap();
        assert_eq!(event.event_name, "line_deleted");
        assert_eq!(event.id, first.line_seq);
        assert!(matches!(
            event.payload,
            BrowserEvent::LineDeleted(bridge_protocol::BrowserLineDeletedEvent { line_id })
                if line_id == first.line_id
        ));
        assert!(!state.delete_line(first.line_id).unwrap());
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

    fn line_record(
        line_id: LineId,
        text: &str,
        process_id: u32,
        thread_number: i64,
        audio: Option<AudioState>,
    ) -> LineRecord {
        LineRecord {
            line_id,
            line_seq: line_id,
            timestamp_unix_ms: 10_000,
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
            screenshot: None,
            audio,
            warnings: Vec::new(),
        }
    }

    fn samples_by_ms(duration_ms: u64) -> Vec<i16> {
        vec![0; duration_ms as usize * 48]
    }

    fn main_recording_ids(manager: &AudioManager, process_id: u32) -> Vec<LineId> {
        manager.main_recording_line_ids_for_process(process_id)
    }
}
