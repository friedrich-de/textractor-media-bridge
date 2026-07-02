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

              <label
                v-for="field in normalAudioFields"
                :key="field.key"
                class="field compact"
                :class="{ 'span-two': field.spanTwo }"
              >
                <span>{{ field.label }}</span>
                <input
                  v-model.number="localAudioConfig[field.key]"
                  :name="field.name"
                  type="number"
                  :min="field.min"
                  :max="field.max"
                  :step="field.step"
                />
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Audio capture (extended trim)</h3>
            </div>

            <div class="settings-grid">
              <label
                v-for="field in trimAudioFields"
                :key="field.key"
                class="field compact"
                :class="{ 'span-two': field.spanTwo }"
              >
                <span>{{ field.label }}</span>
                <input
                  v-model.number="localAudioConfig[field.key]"
                  :name="field.name"
                  type="number"
                  :min="field.min"
                  :max="field.max"
                  :step="field.step"
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

import { getModelsWithFields } from '@/api/ankiConnect';
import type { AudioConfig } from '@/api/types';
import {
  cloneAudioConfig,
  defaultAudioConfig,
  normalAudioFields,
  normalizeAudioConfig,
  trimAudioFields,
} from '@/lib/audioSettings';
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

const localSettings = reactive<MinerSettings>(cloneMinerSettings(props.settings));
const localAudioConfig = reactive<AudioConfig>(
  cloneAudioConfig(props.audioConfig ?? defaultAudioConfig),
);
const connectionStatus = ref<ConnectionStatus>('untested');
const connectionError = ref<string | null>(null);
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
    return 'Connected to AnkiConnect';
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
    await loadModels();
    connectionStatus.value = 'connected';
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
    audioConfig: normalizeAudioConfig(localAudioConfig),
  });
}
</script>
