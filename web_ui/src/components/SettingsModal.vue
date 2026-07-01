<template>
  <Teleport to="body">
    <div class="modal-overlay" @click.self="emit('cancel')">
      <section
        class="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <header class="modal-header">
          <div>
            <p>Mining</p>
            <h2 id="settings-title">Settings</h2>
          </div>
          <button
            class="icon-button"
            type="button"
            aria-label="Close settings"
            @click="emit('cancel')"
          >
            <X :size="18" />
          </button>
        </header>

        <div class="modal-body">
          <section class="settings-section">
            <div class="section-heading">
              <h3>AnkiConnect</h3>
              <button
                class="secondary-action"
                type="button"
                :disabled="connectionStatus === 'testing'"
                @click="testConnection"
              >
                <LoaderCircle v-if="connectionStatus === 'testing'" class="spin" :size="16" />
                <PlugZap v-else :size="16" />
                <span>{{ connectionStatus === 'testing' ? 'Testing' : 'Test' }}</span>
              </button>
            </div>

            <div class="connection-status" :data-state="connectionStatus">
              <span class="dot" aria-hidden="true"></span>
              <span>{{ connectionLabel }}</span>
            </div>

            <label class="field compact">
              <span>Endpoint</span>
              <input
                v-model="localSettings.anki.endpoint"
                name="anki_endpoint"
                autocomplete="off"
              />
            </label>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Card fields</h3>
              <button
                class="secondary-action ghost"
                type="button"
                :disabled="connectionStatus !== 'connected'"
                @click="loadModels"
              >
                <RefreshCw :size="16" />
                <span>Reload</span>
              </button>
            </div>

            <div class="settings-grid">
              <label class="field compact">
                <span>Deck</span>
                <input v-model="localSettings.anki.deckName" name="anki_deck" autocomplete="off" />
              </label>

              <label class="field compact">
                <span>Note type</span>
                <select
                  v-model="localSettings.anki.modelName"
                  name="anki_note_type"
                  @change="applyFieldDefaults"
                >
                  <option :value="localSettings.anki.modelName">
                    {{ localSettings.anki.modelName || 'Lapis' }}
                  </option>
                  <option v-for="model in modelNames" :key="model" :value="model">
                    {{ model }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Front field</span>
                <select v-model="localSettings.anki.frontField" name="anki_front_field">
                  <option value="">Skip</option>
                  <option v-for="field in availableFields" :key="field" :value="field">
                    {{ field }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Sentence field</span>
                <select v-model="localSettings.anki.sentenceField" name="anki_sentence_field">
                  <option value="">Skip</option>
                  <option v-for="field in availableFields" :key="field" :value="field">
                    {{ field }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Audio field</span>
                <select v-model="localSettings.anki.audioField" name="anki_audio_field">
                  <option value="">Skip</option>
                  <option v-for="field in availableFields" :key="field" :value="field">
                    {{ field }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Image field</span>
                <select v-model="localSettings.anki.imageField" name="anki_image_field">
                  <option value="">Skip</option>
                  <option v-for="field in availableFields" :key="field" :value="field">
                    {{ field }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Source field</span>
                <select v-model="localSettings.anki.sourceField" name="anki_source_field">
                  <option value="">Skip</option>
                  <option v-for="field in availableFields" :key="field" :value="field">
                    {{ field }}
                  </option>
                </select>
              </label>

              <label class="field compact">
                <span>Max card age (minutes)</span>
                <input
                  v-model.number="localSettings.anki.maxLatestCardAgeMinutes"
                  name="anki_max_latest_card_age_minutes"
                  type="number"
                  min="0"
                  step="0.1"
                />
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Behavior</h3>
              <button class="secondary-action ghost" type="button" @click="resetToLapis">
                <RotateCcw :size="16" />
                <span>Lapis defaults</span>
              </button>
            </div>

            <div class="settings-grid">
              <label class="field compact">
                <span>Range screenshot</span>
                <select
                  v-model="localSettings.anki.rangeScreenshotPick"
                  name="anki_range_screenshot_pick"
                >
                  <option value="last">Last line</option>
                  <option value="first">First line</option>
                </select>
              </label>

              <label class="field compact span-two">
                <span>Range separator</span>
                <input
                  v-model="localSettings.anki.rangeSentenceSeparator"
                  name="anki_range_sentence_separator"
                  autocomplete="off"
                />
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Audio capture (normal)</h3>
              <button class="secondary-action ghost" type="button" @click="resetAudioDefaults">
                <RotateCcw :size="16" />
                <span>Audio defaults</span>
              </button>
            </div>

            <div class="settings-grid">
              <label class="field compact">
                <span>Backend</span>
                <select v-model="localAudioConfig.backend" name="audio_backend">
                  <option value="auto">Auto</option>
                  <option value="process-loopback">Process loopback</option>
                  <option value="system-loopback">System loopback</option>
                  <option value="off">Off</option>
                </select>
              </label>

              <label class="field compact">
                <span>Activity threshold</span>
                <input
                  v-model.number="localAudioConfig.activity_threshold"
                  name="audio_activity_threshold"
                  type="number"
                  min="1"
                  max="30000"
                  step="1"
                />
              </label>

              <label class="field compact">
                <span>Minimum activity (ms)</span>
                <input
                  v-model.number="localAudioConfig.min_activity_ms"
                  name="audio_min_activity_ms"
                  type="number"
                  min="1"
                  max="1000"
                  step="1"
                />
              </label>

              <label class="field compact">
                <span>Pre-roll (ms)</span>
                <input
                  v-model.number="localAudioConfig.ready_preroll_ms"
                  name="audio_ready_preroll_ms"
                  type="number"
                  min="0"
                  max="5000"
                  step="100"
                />
              </label>

              <label class="field compact">
                <span>Trailing silence (ms)</span>
                <input
                  v-model.number="localAudioConfig.trailing_silence_ms"
                  name="audio_trailing_silence_ms"
                  type="number"
                  min="100"
                  max="15000"
                  step="100"
                />
              </label>

              <label class="field compact">
                <span>No speech timeout (ms)</span>
                <input
                  v-model.number="localAudioConfig.no_speech_timeout_ms"
                  name="audio_no_speech_timeout_ms"
                  type="number"
                  min="500"
                  max="30000"
                  step="250"
                />
              </label>

              <label class="field compact">
                <span>Auto trim padding (ms)</span>
                <input
                  v-model.number="localAudioConfig.trim_padding_ms"
                  name="audio_trim_padding_ms"
                  type="number"
                  min="0"
                  max="5000"
                  step="100"
                />
              </label>

              <label class="field compact">
                <span>Max duration (ms)</span>
                <input
                  v-model.number="localAudioConfig.max_duration_ms"
                  name="audio_max_duration_ms"
                  type="number"
                  min="1000"
                  max="300000"
                  step="1000"
                />
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Audio capture (extended trim)</h3>
            </div>

            <div class="settings-grid">
              <label class="field compact">
                <span>Activity threshold</span>
                <input
                  v-model.number="localAudioConfig.trim_activity_threshold"
                  name="audio_trim_activity_threshold"
                  type="number"
                  min="1"
                  max="30000"
                  step="1"
                />
              </label>

              <label class="field compact">
                <span>Minimum activity (ms)</span>
                <input
                  v-model.number="localAudioConfig.trim_min_activity_ms"
                  name="audio_trim_min_activity_ms"
                  type="number"
                  min="1"
                  max="1000"
                  step="1"
                />
              </label>

              <label class="field compact">
                <span>Pre-roll (ms)</span>
                <input
                  v-model.number="localAudioConfig.trim_source_preroll_ms"
                  name="audio_trim_source_preroll_ms"
                  type="number"
                  min="0"
                  max="5000"
                  step="100"
                />
              </label>

              <label class="field compact">
                <span>Trailing silence (ms)</span>
                <input
                  v-model.number="localAudioConfig.trim_trailing_silence_ms"
                  name="audio_trim_trailing_silence_ms"
                  type="number"
                  min="500"
                  max="20000"
                  step="250"
                />
              </label>

              <label class="field compact span-two">
                <span>No speech timeout (ms)</span>
                <input
                  v-model.number="localAudioConfig.trim_no_speech_timeout_ms"
                  name="audio_trim_no_speech_timeout_ms"
                  type="number"
                  min="1000"
                  max="60000"
                  step="500"
                />
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Media</h3>
            </div>
            <div class="media-note">
              Mining payloads use JPEG screenshots and MP3 audio. The server prepares those assets
              before the browser sends them to AnkiConnect.
            </div>
          </section>
        </div>

        <footer class="modal-footer">
          <button class="secondary-action ghost" type="button" @click="emit('cancel')">
            Cancel
          </button>
          <button class="primary-action modal-save" type="button" @click="save">Save</button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from 'vue';
import { LoaderCircle, PlugZap, RefreshCw, RotateCcw, X } from 'lucide-vue-next';

import { getModelsWithFields, getVersion } from '@/api/ankiConnect';
import type { AudioConfig } from '@/api/types';
import type { MinerSettings } from '@/lib/minerSettings';
import { cloneMinerSettings, defaultMinerSettings } from '@/lib/minerSettings';

const props = defineProps<{
  settings: MinerSettings;
  audioConfig: AudioConfig | null;
}>();

type SettingsSavePayload = {
  settings: MinerSettings;
  audioConfig: AudioConfig;
};

const emit = defineEmits<{
  save: [payload: SettingsSavePayload];
  cancel: [];
}>();

type ConnectionStatus = 'untested' | 'testing' | 'connected' | 'error';

const defaultAudioConfig: AudioConfig = {
  backend: 'auto',
  vad: 'webrtc',
  format: 'wav',
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

const localSettings = reactive<MinerSettings>(cloneMinerSettings(props.settings));
const localAudioConfig = reactive<AudioConfig>(
  cloneAudioConfig(props.audioConfig ?? defaultAudioConfig),
);
const connectionStatus = ref<ConnectionStatus>('untested');
const connectionError = ref<string | null>(null);
const ankiVersion = ref<number | null>(null);
const modelsWithFields = ref<Record<string, string[]>>({});

const modelNames = computed(() => Object.keys(modelsWithFields.value).sort());
const availableFields = computed(() => {
  const fromAnki = modelsWithFields.value[localSettings.anki.modelName] ?? [];
  return fromAnki.length > 0
    ? fromAnki
    : [
        localSettings.anki.frontField,
        localSettings.anki.sentenceField,
        localSettings.anki.audioField,
        localSettings.anki.imageField,
        localSettings.anki.sourceField,
      ].filter(Boolean);
});
const connectionLabel = computed(() => {
  if (connectionStatus.value === 'connected') {
    return `Connected to AnkiConnect v${ankiVersion.value}`;
  }

  if (connectionStatus.value === 'error') {
    return connectionError.value ?? 'Unable to connect';
  }

  if (connectionStatus.value === 'testing') {
    return `Testing ${localSettings.anki.endpoint}`;
  }

  return 'Not tested';
});

async function testConnection(): Promise<void> {
  connectionStatus.value = 'testing';
  connectionError.value = null;
  try {
    ankiVersion.value = await getVersion(localSettings.anki.endpoint);
    connectionStatus.value = 'connected';
    await loadModels();
  } catch (error) {
    connectionStatus.value = 'error';
    connectionError.value = error instanceof Error ? error.message : 'Unable to connect';
  }
}

async function loadModels(): Promise<void> {
  modelsWithFields.value = await getModelsWithFields(localSettings.anki.endpoint);
  applyModelDefault();
  applyFieldDefaults();
}

function applyModelDefault(): void {
  if (!localSettings.anki.modelName && modelsWithFields.value.Lapis) {
    localSettings.anki.modelName = 'Lapis';
  }
}

function applyFieldDefaults(): void {
  const fields = modelsWithFields.value[localSettings.anki.modelName] ?? [];
  for (const [setting, fieldName] of Object.entries({
    frontField: 'Expression',
    sentenceField: 'Sentence',
    audioField: 'SentenceAudio',
    imageField: 'Picture',
    sourceField: 'Source',
  })) {
    const key = setting as keyof MinerSettings['anki'];
    if (!localSettings.anki[key] && fields.includes(fieldName)) {
      localSettings.anki[key] = fieldName as never;
    }
  }
}

function resetToLapis(): void {
  localSettings.anki = {
    ...localSettings.anki,
    ...defaultMinerSettings.anki,
    endpoint: localSettings.anki.endpoint,
    deckName: localSettings.anki.deckName,
  };
}

function resetAudioDefaults(): void {
  Object.assign(localAudioConfig, defaultAudioConfig);
}

function save(): void {
  emit('save', {
    settings: cloneMinerSettings(localSettings),
    audioConfig: normalizedAudioConfig(),
  });
}

function cloneAudioConfig(config: AudioConfig): AudioConfig {
  return { ...defaultAudioConfig, ...config };
}

function normalizedAudioConfig(): AudioConfig {
  return {
    ...localAudioConfig,
    backend: localAudioConfig.backend || defaultAudioConfig.backend,
    vad: localAudioConfig.vad || defaultAudioConfig.vad,
    format: localAudioConfig.format || defaultAudioConfig.format,
    ready_preroll_ms: clampInteger(localAudioConfig.ready_preroll_ms, 0, 5_000),
    trailing_silence_ms: clampInteger(localAudioConfig.trailing_silence_ms, 100, 15_000),
    no_speech_timeout_ms: clampInteger(localAudioConfig.no_speech_timeout_ms, 500, 30_000),
    trim_source_preroll_ms: clampInteger(localAudioConfig.trim_source_preroll_ms, 0, 5_000),
    trim_trailing_silence_ms: clampInteger(localAudioConfig.trim_trailing_silence_ms, 500, 20_000),
    trim_no_speech_timeout_ms: clampInteger(
      localAudioConfig.trim_no_speech_timeout_ms,
      1_000,
      60_000,
    ),
    activity_threshold: clampInteger(localAudioConfig.activity_threshold, 1, 30_000),
    min_activity_ms: clampInteger(localAudioConfig.min_activity_ms, 1, 1_000),
    trim_activity_threshold: clampInteger(localAudioConfig.trim_activity_threshold, 1, 30_000),
    trim_min_activity_ms: clampInteger(localAudioConfig.trim_min_activity_ms, 1, 1_000),
    trim_padding_ms: clampInteger(localAudioConfig.trim_padding_ms, 0, 5_000),
    max_duration_ms: clampInteger(localAudioConfig.max_duration_ms, 1_000, 300_000),
  };
}

function clampInteger(value: number, min: number, max: number): number {
  const parsed = Number.isFinite(value) ? Math.trunc(value) : min;
  return Math.min(Math.max(parsed, min), max);
}
</script>
