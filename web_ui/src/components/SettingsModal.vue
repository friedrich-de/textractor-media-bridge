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
          <TooltipButton
            class="icon-button"
            type="button"
            tooltip="Close settings"
            @click="emit('cancel')"
          >
            <X :size="18" />
          </TooltipButton>
        </header>

        <div class="modal-body">
          <section class="settings-section">
            <div class="section-heading">
              <h3>AnkiConnect</h3>
              <button
                class="secondary-action"
                type="button"
                :disabled="!hasAnkiEndpoint || connectionStatus === 'testing'"
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
                :disabled="!hasAnkiEndpoint || connectionStatus === 'testing'"
                @click="loadModels"
              >
                <RefreshCw :size="16" />
                <span>Reload</span>
              </button>
            </div>

            <div class="settings-grid">
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

              <label class="check-field span-two">
                <input
                  v-model="localLineConfig.joinProgressiveText"
                  name="lines_join_progressive_text"
                  type="checkbox"
                />
                <span>Join progressive text updates</span>
              </label>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Text filters</h3>
              <button class="secondary-action ghost" type="button" @click="addRegexFilter">
                <Plus :size="16" />
                <span>Add</span>
              </button>
            </div>

            <label class="check-field">
              <input
                v-model="localSettings.textFilters.deduplicateMultilinePrefixes"
                name="text_filters_deduplicate_multiline_prefixes"
                type="checkbox"
              />
              <span>Deduplicate progressive lines inside text</span>
            </label>

            <div v-if="localSettings.textFilters.regexes.length > 0" class="regex-filter-list">
              <div
                v-for="(_regex, index) in localSettings.textFilters.regexes"
                :key="index"
                class="regex-filter-row"
              >
                <label class="field compact regex-filter-field">
                  <span>Regex {{ index + 1 }}</span>
                  <input
                    v-model="localSettings.textFilters.regexes[index]"
                    :name="`text_filter_regex_${index}`"
                    autocomplete="off"
                    spellcheck="false"
                    :aria-invalid="Boolean(regexErrors[index])"
                    @input="clearRegexError(index)"
                  />
                  <span v-if="regexErrors[index]" class="field-error">
                    {{ regexErrors[index] }}
                  </span>
                </label>
                <TooltipButton
                  class="icon-button small danger"
                  type="button"
                  tooltip="Remove regex filter"
                  @click="removeRegexFilter(index)"
                >
                  <Trash2 :size="16" />
                </TooltipButton>
              </div>
            </div>
          </section>

          <section class="settings-section">
            <div class="section-heading">
              <h3>Audio capture</h3>
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
import { computed, onMounted, reactive, ref } from 'vue';
import { LoaderCircle, PlugZap, Plus, RefreshCw, RotateCcw, Trash2, X } from '@lucide/vue';

import { getModelsWithFields } from '@/api/ankiConnect';
import type { AudioConfig, LineConfig } from '@/api/types';
import type { MinerSettings } from '@/lib/minerSettings';
import TooltipButton from '@/components/TooltipButton.vue';
import { cloneMinerSettings, defaultMinerSettings } from '@/lib/minerSettings';
import {
  hasTextFilterErrors,
  normalizeTextFilterPatterns,
  validateTextFilterPatterns,
} from '@/lib/textFilters';

const defaultAudioConfig: AudioConfig = {
  backend: 'auto',
};

const defaultLineConfig: LineConfig = {
  joinProgressiveText: true,
};

const props = defineProps<{
  settings: MinerSettings;
  audioConfig: AudioConfig | null;
  lineConfig: LineConfig | null;
}>();

type SettingsSavePayload = {
  settings: MinerSettings;
  audioConfig: AudioConfig;
  lineConfig: LineConfig;
};

const emit = defineEmits<{
  save: [payload: SettingsSavePayload];
  cancel: [];
}>();

type ConnectionStatus = 'untested' | 'testing' | 'connected' | 'error';
type AnkiFieldSetting =
  'frontField' | 'sentenceField' | 'audioField' | 'imageField' | 'sourceField';

const ankiFieldDefaults: Record<AnkiFieldSetting, string> = {
  frontField: 'Expression',
  sentenceField: 'Sentence',
  audioField: 'SentenceAudio',
  imageField: 'Picture',
  sourceField: 'Source',
};

const localSettings = reactive<MinerSettings>(cloneMinerSettings(props.settings));
const localAudioConfig = reactive<AudioConfig>(
  cloneAudioConfig(props.audioConfig ?? defaultAudioConfig),
);
const localLineConfig = reactive<LineConfig>(
  cloneLineConfig(props.lineConfig ?? defaultLineConfig),
);
const connectionStatus = ref<ConnectionStatus>('untested');
const connectionError = ref<string | null>(null);
const modelsWithFields = ref<Record<string, string[]>>({});
const fieldsLoaded = ref(false);
const regexErrors = ref<Array<string | null>>([]);
let modelLoadRequestId = 0;

const modelNames = computed(() => Object.keys(modelsWithFields.value).sort());
const hasAnkiEndpoint = computed(() => localSettings.anki.endpoint.trim().length > 0);
const availableFields = computed(() => {
  if (fieldsLoaded.value) {
    return modelsWithFields.value[localSettings.anki.modelName] ?? [];
  }

  return [
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

onMounted(() => {
  if (hasAnkiEndpoint.value) {
    void loadModels();
  }
});

async function testConnection(): Promise<void> {
  await loadModels();
}

async function loadModels(): Promise<void> {
  const requestId = ++modelLoadRequestId;
  const endpoint = localSettings.anki.endpoint.trim();
  if (!endpoint) {
    connectionStatus.value = 'error';
    connectionError.value = 'AnkiConnect endpoint is required';
    modelsWithFields.value = {};
    fieldsLoaded.value = false;
    return;
  }

  connectionStatus.value = 'testing';
  connectionError.value = null;
  try {
    const nextModelsWithFields = await getModelsWithFields(endpoint);
    if (requestId !== modelLoadRequestId) {
      return;
    }
    modelsWithFields.value = nextModelsWithFields;
    fieldsLoaded.value = true;
    applyModelDefault();
    applyFieldDefaults();
    connectionStatus.value = 'connected';
  } catch (error) {
    if (requestId !== modelLoadRequestId) {
      return;
    }
    connectionStatus.value = 'error';
    connectionError.value = error instanceof Error ? error.message : 'Unable to connect';
  }
}

function applyModelDefault(): void {
  if (!localSettings.anki.modelName && modelsWithFields.value.Lapis) {
    localSettings.anki.modelName = 'Lapis';
  }
}

function applyFieldDefaults(): void {
  const fields = modelsWithFields.value[localSettings.anki.modelName] ?? [];
  const fieldSet = new Set(fields);

  for (const [setting, fieldName] of Object.entries(ankiFieldDefaults)) {
    const key = setting as AnkiFieldSetting;
    const currentField = localSettings.anki[key].trim();
    if (currentField && fields.length > 0 && !fieldSet.has(currentField)) {
      localSettings.anki[key] = '';
    }
    if (!localSettings.anki[key] && fieldSet.has(fieldName)) {
      localSettings.anki[key] = fieldName;
    }
  }
}

function resetToLapis(): void {
  localSettings.anki = {
    ...localSettings.anki,
    ...defaultMinerSettings.anki,
    endpoint: localSettings.anki.endpoint,
  };
}

function addRegexFilter(): void {
  localSettings.textFilters.regexes.push('');
}

function removeRegexFilter(index: number): void {
  localSettings.textFilters.regexes.splice(index, 1);
  regexErrors.value.splice(index, 1);
}

function clearRegexError(index: number): void {
  regexErrors.value[index] = null;
}

function save(): void {
  regexErrors.value = validateTextFilterPatterns(localSettings.textFilters.regexes);
  if (hasTextFilterErrors(regexErrors.value)) {
    return;
  }

  const normalizedSettings = cloneMinerSettings(localSettings);
  normalizedSettings.textFilters.regexes = normalizeTextFilterPatterns(
    normalizedSettings.textFilters.regexes,
  );

  emit('save', {
    settings: normalizedSettings,
    audioConfig: normalizeAudioConfig(localAudioConfig),
    lineConfig: normalizeLineConfig(localLineConfig),
  });
}

function cloneAudioConfig(config: Partial<AudioConfig> = {}): AudioConfig {
  return { ...defaultAudioConfig, ...config };
}

function normalizeAudioConfig(config: AudioConfig): AudioConfig {
  const normalized = cloneAudioConfig(config);
  normalized.backend = normalized.backend || defaultAudioConfig.backend;
  return normalized;
}

function cloneLineConfig(config: Partial<LineConfig> = {}): LineConfig {
  return { ...defaultLineConfig, ...config };
}

function normalizeLineConfig(config: LineConfig): LineConfig {
  return cloneLineConfig(config);
}
</script>
