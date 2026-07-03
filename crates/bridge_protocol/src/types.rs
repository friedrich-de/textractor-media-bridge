use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

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
    #[serde(default)]
    pub ignored: bool,
}

impl LineRecord {
    pub fn source_key(&self) -> String {
        source_key(&self.meta)
    }
}

pub fn source_key(meta: &PipeLineMeta) -> String {
    format!(
        "{}:{}",
        meta.process_id,
        meta.thread_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or("unknown")
    )
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<Option<AssetInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Option<AudioState>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserEvent {
    Hello(BrowserHello),
    LineAdded(BrowserLineAddedEvent),
    LineUpdated(LineUpdatedEvent),
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
        #[serde(rename = "startedUnixMs", alias = "started_unix_ms")]
        started_unix_ms: i64,
    },
    Ready {
        asset: AssetInfo,
        #[serde(rename = "durationMs", alias = "duration_ms")]
        duration_ms: u64,
        #[serde(rename = "endReason", alias = "end_reason")]
        end_reason: AudioEndReason,
        #[serde(
            default,
            rename = "trimSource",
            alias = "trim_source",
            skip_serializing_if = "Option::is_none"
        )]
        trim_source: Option<AudioTrimSource>,
        #[serde(
            default,
            rename = "trimRecordingStartedUnixMs",
            alias = "trim_recording_started_unix_ms",
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
    Silence,
    NoSpeechTimeout,
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
