import type { MiningMode, RangeScreenshotPick } from '@/api/types';

export interface AnkiSettings {
  endpoint: string;
  deckName: string;
  modelName: string;
  frontField: string;
  sentenceField: string;
  notesField: string;
  audioField: string;
  imageField: string;
  sourceField: string;
  maxLatestCardAgeMinutes: number;
  fallbackCreateNote: boolean;
  mode: MiningMode;
  rangeSentenceSeparator: string;
  rangeScreenshotPick: RangeScreenshotPick;
  tags: string[];
}

export interface MinerSettings {
  anki: AnkiSettings;
}

export const defaultMinerSettings: MinerSettings = {
  anki: {
    endpoint: 'http://127.0.0.1:8765',
    deckName: 'Mining',
    modelName: 'Lapis',
    frontField: 'Expression',
    sentenceField: 'Sentence',
    notesField: 'Notes',
    audioField: 'SentenceAudio',
    imageField: 'Picture',
    sourceField: 'Source',
    maxLatestCardAgeMinutes: 5,
    fallbackCreateNote: false,
    mode: 'update_latest',
    rangeSentenceSeparator: ' ',
    rangeScreenshotPick: 'last',
    tags: ['textractor', 'mined'],
  },
};

const STORAGE_KEY = 'textractor-media-bridge.mining-settings';

export function loadMinerSettings(): MinerSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      return mergeSettings(JSON.parse(raw) as Partial<MinerSettings>);
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

export function settingsFromServer(serverSettings: Partial<AnkiSettings>): MinerSettings {
  return mergeSettings({ anki: serverSettings });
}

function mergeSettings(settings: { anki?: Partial<AnkiSettings> }): MinerSettings {
  return {
    anki: {
      ...defaultMinerSettings.anki,
      ...settings.anki,
    },
  };
}
