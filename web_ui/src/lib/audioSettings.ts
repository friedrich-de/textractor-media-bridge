import type { AudioConfig } from '@/api/types';

export const defaultAudioConfig: AudioConfig = {
  backend: 'auto',
  ready_preroll_ms: 1_000,
  trailing_silence_ms: 3_000,
  no_speech_timeout_ms: 5_000,
  trim_source_preroll_ms: 1_000,
  trim_trailing_silence_ms: 5_000,
  trim_no_speech_timeout_ms: 8_000,
  activity_threshold: 300,
  min_activity_ms: 30,
  trim_activity_threshold: 300,
  trim_min_activity_ms: 30,
  trim_padding_ms: 1_000,
  max_duration_ms: 120_000,
};

type AudioNumberKey = Exclude<keyof AudioConfig, 'backend'>;

export interface AudioNumberField {
  key: AudioNumberKey;
  label: string;
  name: string;
  min: number;
  max: number;
  step: number;
  spanTwo?: boolean;
}

export const normalAudioFields = [
  numberField('activity_threshold', 'Activity threshold', 'audio_activity_threshold', 1, 30_000, 1),
  numberField('min_activity_ms', 'Minimum activity (ms)', 'audio_min_activity_ms', 1, 1_000, 1),
  numberField('ready_preroll_ms', 'Pre-roll (ms)', 'audio_ready_preroll_ms', 0, 5_000, 100),
  numberField(
    'trailing_silence_ms',
    'Trailing silence (ms)',
    'audio_trailing_silence_ms',
    100,
    15_000,
    100,
  ),
  numberField(
    'no_speech_timeout_ms',
    'No speech timeout (ms)',
    'audio_no_speech_timeout_ms',
    500,
    30_000,
    250,
  ),
  numberField('trim_padding_ms', 'Auto trim padding (ms)', 'audio_trim_padding_ms', 0, 5_000, 100),
  numberField(
    'max_duration_ms',
    'Max duration (ms)',
    'audio_max_duration_ms',
    1_000,
    300_000,
    1_000,
  ),
] as const satisfies readonly AudioNumberField[];

export const trimAudioFields = [
  numberField(
    'trim_activity_threshold',
    'Activity threshold',
    'audio_trim_activity_threshold',
    1,
    30_000,
    1,
  ),
  numberField(
    'trim_min_activity_ms',
    'Minimum activity (ms)',
    'audio_trim_min_activity_ms',
    1,
    1_000,
    1,
  ),
  numberField(
    'trim_source_preroll_ms',
    'Pre-roll (ms)',
    'audio_trim_source_preroll_ms',
    0,
    5_000,
    100,
  ),
  numberField(
    'trim_trailing_silence_ms',
    'Trailing silence (ms)',
    'audio_trim_trailing_silence_ms',
    500,
    20_000,
    250,
  ),
  numberField(
    'trim_no_speech_timeout_ms',
    'No speech timeout (ms)',
    'audio_trim_no_speech_timeout_ms',
    1_000,
    60_000,
    500,
    true,
  ),
] as const satisfies readonly AudioNumberField[];

const audioNumberFields = [...normalAudioFields, ...trimAudioFields] as readonly AudioNumberField[];

export function cloneAudioConfig(config: Partial<AudioConfig> = {}): AudioConfig {
  return { ...defaultAudioConfig, ...config };
}

export function normalizeAudioConfig(config: AudioConfig): AudioConfig {
  const normalized = cloneAudioConfig(config);
  normalized.backend = normalized.backend || defaultAudioConfig.backend;
  for (const field of audioNumberFields) {
    normalized[field.key] = clampInteger(normalized[field.key], field.min, field.max);
  }
  return normalized;
}

function numberField(
  key: AudioNumberKey,
  label: string,
  name: string,
  min: number,
  max: number,
  step: number,
  spanTwo = false,
): AudioNumberField {
  return { key, label, name, min, max, step, spanTwo };
}

function clampInteger(value: number, min: number, max: number): number {
  const parsed = Number.isFinite(value) ? Math.trunc(value) : min;
  return Math.min(Math.max(parsed, min), max);
}
