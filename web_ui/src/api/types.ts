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

export type AudioEndReason = 'manual' | 'line_advanced' | 'max_duration' | 'backend_unavailable';

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
}

export interface LinePatch {
  text?: string;
  screenshot?: AssetInfo | null;
  audio?: AudioState | null;
  warnings?: string[];
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
  };
  audio: AudioConfig;
  lines: LineConfig;
  anki: {
    range_sentence_separator: string;
    range_screenshot_pick: RangeScreenshotPick;
  };
}

export interface AudioConfig {
  backend: string;
}

export interface LineConfig {
  joinProgressiveText: boolean;
}

export interface EditableConfigRequest {
  audio: AudioConfig;
  lines: LineConfig;
}

export interface PublicConfig {
  protocolVersion: number;
  config: AppConfig;
  pipeName: string;
  dataDir: string;
}

export type RangeScreenshotPick = 'first' | 'last';

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
