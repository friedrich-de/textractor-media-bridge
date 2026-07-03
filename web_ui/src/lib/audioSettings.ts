import type { AudioConfig } from '@/api/types';

export const defaultAudioConfig: AudioConfig = {
  backend: 'auto',
};

export function cloneAudioConfig(config: Partial<AudioConfig> = {}): AudioConfig {
  return { ...defaultAudioConfig, ...config };
}

export function normalizeAudioConfig(config: AudioConfig): AudioConfig {
  const normalized = cloneAudioConfig(config);
  normalized.backend = normalized.backend || defaultAudioConfig.backend;
  return normalized;
}
