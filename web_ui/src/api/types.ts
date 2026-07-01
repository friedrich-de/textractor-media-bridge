export type LineId = number;
export type LineSeq = number;

export interface PipeLineMeta {
  processId: number;
  threadNumber: number;
  threadName?: string;
  windowTitle?: string;
  isCurrentSelect: boolean;
  arch: string;
  source: string;
}

export type AssetKind = 'screenshot' | 'audio';

export interface AssetInfo {
  assetId: string;
  kind: AssetKind;
  mimeType: string;
  url: string;
  width?: number;
  height?: number;
  durationMs?: number;
  createdUnixMs: number;
  byteSize: number;
}

export type AudioEndReason =
  'manual' | 'silence' | 'no_speech_timeout' | 'max_duration' | 'backend_unavailable';

export interface AudioTrimSource {
  asset: AssetInfo;
  sourceDurationMs: number;
  startMs: number;
  endMs: number;
  canExtend: boolean;
}

export type AudioState =
  | { status: 'recording'; startedUnixMs: number }
  | {
      status: 'ready';
      asset: AssetInfo;
      durationMs: number;
      endReason: AudioEndReason;
      trimSource?: AudioTrimSource | null;
      trimRecordingStartedUnixMs?: number | null;
    }
  | { status: 'no_audio'; reason?: string };

export interface AudioTrimInfoResponse {
  lineId: LineId;
  source: AssetInfo;
  sourceDurationMs: number;
  startMs: number;
  endMs: number;
  canExtend: boolean;
}

export interface AudioTrimRequest {
  startMs: number;
  endMs: number;
}

export interface LineRecord {
  lineId: LineId;
  lineSeq: LineSeq;
  timestampUnixMs: number;
  text: string;
  meta: PipeLineMeta;
  screenshot?: AssetInfo | null;
  audio?: AudioState | null;
  warnings: string[];
  ignored: boolean;
}

export interface LinePatch {
  screenshot?: AssetInfo | null;
  audio?: AudioState | null;
  warnings?: string[];
  ignored?: boolean;
}

export interface LineHistoryPage {
  lines: LineRecord[];
  oldestSeq?: LineSeq | null;
  newestSeq?: LineSeq | null;
  hasMoreOlder: boolean;
  hasMoreNewer: boolean;
}

export interface AppConfig {
  server: {
    bind: string;
    lan_mode: boolean;
    session_token_required: boolean;
  };
  audio: AudioConfig;
  anki: {
    endpoint: string;
    mode: MiningMode;
    max_latest_card_age_minutes: number;
    overwrite_sentence_field: boolean;
    fallback_create_note: boolean;
    range_sentence_separator: string;
    range_screenshot_pick: RangeScreenshotPick;
    deck_name: string;
    model_name: string;
    sentence_field: string;
    notes_field: string;
    screenshot_field: string;
    audio_field: string;
    source_field: string;
    tags: string[];
  };
}

export interface AudioConfig {
  backend: string;
  vad: string;
  format: string;
  ready_preroll_ms: number;
  trailing_silence_ms: number;
  no_speech_timeout_ms: number;
  trim_source_preroll_ms: number;
  trim_trailing_silence_ms: number;
  trim_no_speech_timeout_ms: number;
  activity_threshold: number;
  min_activity_ms: number;
  trim_activity_threshold: number;
  trim_min_activity_ms: number;
  trim_padding_ms: number;
  max_duration_ms: number;
}

export interface PublicConfig {
  protocolVersion: number;
  config: AppConfig;
  pipeName: string;
  dataDir: string;
  sessionTokenRequired: boolean;
  sessionToken?: string | null;
}

export type RangeScreenshotPick = 'first' | 'last';
export type MiningMode = 'update_latest';

export interface MinePrepareRequest {
  lineIds: LineId[];
  rangeSentenceSeparator?: string;
  rangeScreenshotPick?: RangeScreenshotPick;
}

export interface MinePrepareResponse {
  sentence: string;
  screenshot?: AssetInfo | null;
  audio?: AssetInfo | null;
  source: string;
  lineIds: LineId[];
}

export interface AssetBase64Response {
  assetId: string;
  filename: string;
  mimeType: string;
  data: string;
}

export interface BrowserHelloPayload {
  type: 'hello';
  protocolVersion: number;
  serverVersion: string;
  newestSeq?: LineSeq | null;
}

export interface BrowserLineAddedPayload {
  type: 'line_added';
  line: LineRecord;
}

export interface BrowserLineUpdatedPayload {
  type: 'line_updated';
  lineId: LineId;
  lineSeq: LineSeq;
  patch: LinePatch;
}

export interface BrowserErrorPayload {
  type: 'error';
  code: string;
  message: string;
}

export type BrowserEventPayload =
  BrowserHelloPayload | BrowserLineAddedPayload | BrowserLineUpdatedPayload | BrowserErrorPayload;
