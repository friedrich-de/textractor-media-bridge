use anyhow::Result;
use bridge_protocol::{
    AudioState, BrowserEvent, BrowserLineAddedEvent, LineHistoryPage, LineRecord, LineSeq,
    PipeLineEvent, PROTOCOL_VERSION,
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
