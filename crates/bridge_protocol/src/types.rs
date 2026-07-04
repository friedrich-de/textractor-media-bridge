use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 3;

pub type LineId = u64;
pub type LineSeq = u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipeLineEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub protocol_version: u32,
    pub message_id: u64,
    pub timestamp_unix_ms: i64,
    pub text: String,
    pub meta: PipeLineMeta,
}

impl PipeLineEvent {
    pub fn new(message_id: u64, timestamp_unix_ms: i64, text: String, meta: PipeLineMeta) -> Self {
        Self {
            event_type: "line".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            message_id,
            timestamp_unix_ms,
            text,
            meta,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipeLineMeta {
    pub process_id: u32,
    pub thread_number: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    pub is_current_select: bool,
    pub arch: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHello {
    pub protocol_version: u32,
    pub server_version: String,
    pub newest_seq: Option<LineSeq>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRecord {
    pub line_id: LineId,
    pub line_seq: LineSeq,
    pub timestamp_unix_ms: i64,
    pub text: String,
    pub meta: PipeLineMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<AssetInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioState>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub fn thread_label(meta: &PipeLineMeta) -> String {
    meta.thread_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("thread {}", meta.thread_number))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLineAddedEvent {
    pub line: LineRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineUpdatedEvent {
    pub line_id: LineId,
    pub line_seq: LineSeq,
    pub patch: LinePatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesClearedEvent {
    pub cleared_lines: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<Option<AssetInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Option<AudioState>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserEvent {
    Hello(BrowserHello),
    LineAdded(BrowserLineAddedEvent),
    LineUpdated(LineUpdatedEvent),
    LinesCleared(LinesClearedEvent),
    Error(ErrorEvent),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHistoryPage {
    pub lines: Vec<LineRecord>,
    pub oldest_seq: Option<LineSeq>,
    pub newest_seq: Option<LineSeq>,
    pub has_more_older: bool,
    pub has_more_newer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Screenshot,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    pub asset_id: String,
    pub kind: AssetKind,
    pub mime_type: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub created_unix_ms: i64,
    pub byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AudioState {
    Recording {
        #[serde(rename = "startedUnixMs")]
        started_unix_ms: i64,
    },
    Ready {
        asset: AssetInfo,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
        #[serde(rename = "endReason")]
        end_reason: AudioEndReason,
        #[serde(
            default,
            rename = "trimSource",
            skip_serializing_if = "Option::is_none"
        )]
        trim_source: Option<Box<AudioTrimSource>>,
        #[serde(
            default,
            rename = "trimRecordingStartedUnixMs",
            skip_serializing_if = "Option::is_none"
        )]
        trim_recording_started_unix_ms: Option<i64>,
    },
    NoAudio {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrimSource {
    pub asset: AssetInfo,
    pub source_duration_ms: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default)]
    pub can_extend: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEndReason {
    Manual,
    LineAdvanced,
    MaxDuration,
    BackendUnavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFinishResponse {
    pub line_id: LineId,
    pub audio: Option<AudioState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrimRequest {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrimInfoResponse {
    pub line_id: LineId,
    pub source: AssetInfo,
    pub source_duration_ms: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub can_extend: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEvent {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinePrepareRequest {
    pub line_ids: Vec<LineId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_sentence_separator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_screenshot_pick: Option<RangeScreenshotPick>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeScreenshotPick {
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinePrepareResponse {
    pub sentence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<AssetInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AssetInfo>,
    pub source: String,
    pub line_ids: Vec<LineId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBase64Response {
    pub asset_id: String,
    pub filename: String,
    pub mime_type: String,
    pub data: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_label_falls_back_to_thread_number() {
        let meta = PipeLineMeta {
            process_id: 1234,
            thread_number: 17,
            thread_name: None,
            window_title: Some("Game Window".to_owned()),
            is_current_select: true,
            arch: "x86".to_owned(),
            source: "textractor".to_owned(),
        };

        assert_eq!(thread_label(&meta), "thread 17");
    }

    #[test]
    fn browser_event_serializes_lines_cleared() {
        let event = BrowserEvent::LinesCleared(LinesClearedEvent { cleared_lines: 3 });

        let json = serde_json::to_string(&event).expect("event serializes");

        assert_eq!(json, r#"{"type":"lines_cleared","clearedLines":3}"#);
        let decoded: BrowserEvent = serde_json::from_str(&json).expect("event deserializes");
        assert_eq!(decoded, event);
    }

    #[test]
    fn line_patch_serializes_text_patch() {
        let patch = LinePatch {
            text: Some("俺はすごい".to_owned()),
            ..LinePatch::default()
        };

        let json = serde_json::to_string(&patch).expect("patch serializes");

        assert_eq!(json, r#"{"text":"俺はすごい"}"#);
        let decoded: LinePatch = serde_json::from_str(&json).expect("patch deserializes");
        assert_eq!(decoded.text.as_deref(), Some("俺はすごい"));
    }
}
