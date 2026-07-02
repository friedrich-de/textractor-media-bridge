import type { RangeScreenshotPick } from '@/api/types';

export interface AnkiSettings {
  endpoint: string;
  deckName: string;
  modelName: string;
  frontField: string;
  sentenceField: string;
  audioField: string;
  imageField: string;
  sourceField: string;
  maxLatestCardAgeMinutes: number;
  rangeSentenceSeparator: string;
  rangeScreenshotPick: RangeScreenshotPick;
}

export interface MinerSettings {
  anki: AnkiSettings;
}

const DEFAULT_ANKI_ENDPOINT = 'http://127.0.0.1:8765';

export const defaultMinerSettings: MinerSettings = {
  anki: {
    endpoint: DEFAULT_ANKI_ENDPOINT,
    deckName: 'Mining',
    modelName: 'Lapis',
    frontField: 'Expression',
    sentenceField: 'Sentence',
    audioField: 'SentenceAudio',
    imageField: 'Picture',
    sourceField: 'Source',
    maxLatestCardAgeMinutes: 5,
    rangeSentenceSeparator: ' ',
    rangeScreenshotPick: 'last',
  },
};

const STORAGE_KEY = 'textractor-media-bridge.mining-settings';

export function loadMinerSettings(): MinerSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const merged = mergeSettings(JSON.parse(raw) as Partial<MinerSettings>);
      saveMinerSettings(merged);
      return merged;
    }
  } catch {
    localStorage.removeItem(STORAGE_KEY);
  }

  return mergeSettings({});
}

export function saveMinerSettings(settings: MinerSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

export function cloneMinerSettings(settings: MinerSettings): MinerSettings {
  return JSON.parse(JSON.stringify(settings)) as MinerSettings;
}

function mergeSettings(settings: { anki?: Partial<AnkiSettings> }): MinerSettings {
  const anki = {
    ...defaultMinerSettings.anki,
    ...settings.anki,
  };

  return {
    anki: {
      endpoint: normalizeAnkiEndpoint(anki.endpoint),
      deckName: anki.deckName,
      modelName: anki.modelName,
      frontField: anki.frontField,
      sentenceField: anki.sentenceField,
      audioField: anki.audioField,
      imageField: anki.imageField,
      sourceField: anki.sourceField,
      maxLatestCardAgeMinutes: anki.maxLatestCardAgeMinutes,
      rangeSentenceSeparator: anki.rangeSentenceSeparator,
      rangeScreenshotPick: anki.rangeScreenshotPick,
    },
  };
}

function normalizeAnkiEndpoint(endpoint: string): string {
  return endpoint === oldPageHostEndpoint() ? DEFAULT_ANKI_ENDPOINT : endpoint;
}

function oldPageHostEndpoint(): string | null {
  const host = window.location.hostname;
  if (host && host !== '127.0.0.1' && host !== 'localhost' && host !== '::1') {
    return `http://${host}:8765`;
  }
  return null;
}
