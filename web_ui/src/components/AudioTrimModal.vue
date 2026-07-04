<template>
  <Teleport to="body">
    <div class="modal-overlay" @click.self="handleOverlayClick">
      <section
        class="settings-modal audio-trim-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="audio-trim-title"
      >
        <header class="modal-header">
          <div>
            <p>Audio</p>
            <h2 id="audio-trim-title">Trim</h2>
          </div>
          <button class="icon-button" type="button" aria-label="Close trim" @click="emit('close')">
            <X :size="18" />
          </button>
        </header>

        <div class="modal-body audio-trim-body">
          <div v-if="loading" class="trim-status">
            <LoaderCircle class="spin" :size="18" />
            <span>Loading</span>
          </div>

          <div v-else-if="error" class="trim-status warning">
            {{ error }}
          </div>

          <template v-else-if="trimInfo">
            <audio
              ref="audioElement"
              class="trim-audio-element"
              preload="auto"
              @timeupdate="handleTimeUpdate"
              @ended="handlePlaybackEnded"
            ></audio>

            <div class="trim-readout">
              <button class="secondary-action" type="button" @click="togglePlayback">
                <Pause v-if="playing" :size="16" />
                <Play v-else :size="16" />
                <span>{{ playing ? 'Pause' : 'Play' }}</span>
              </button>

              <div class="trim-duration">
                <span>Duration</span>
                <strong>{{ selectedDurationMs }} ms</strong>
              </div>
            </div>

            <div ref="activityTrack" class="trim-activity-track">
              <div class="trim-activity-bars" aria-hidden="true">
                <span
                  v-for="(bar, index) in activityBars"
                  :key="index"
                  class="trim-activity-bar"
                  :class="{ voiced: bar > 0.18 }"
                  :style="activityBarStyle(bar)"
                ></span>
              </div>

              <div class="trim-shade left" :style="leftShadeStyle"></div>
              <div class="trim-shade right" :style="rightShadeStyle"></div>
              <div class="trim-selection-window" :style="selectionStyle"></div>
              <div v-if="playing" class="trim-playhead" :style="playheadStyle"></div>

              <button
                class="trim-handle start"
                type="button"
                :style="startHandleStyle"
                aria-label="Trim left edge"
                @pointerdown.prevent.stop="beginHandleDrag('start', $event)"
              ></button>
              <button
                class="trim-handle end"
                type="button"
                :style="endHandleStyle"
                aria-label="Trim right edge"
                @pointerdown.prevent.stop="beginHandleDrag('end', $event)"
              ></button>
            </div>

            <div class="settings-grid">
              <label class="field compact">
                <span>Start</span>
                <input
                  v-model.number="startMs"
                  name="trim_start_ms"
                  type="number"
                  :min="minStartMs"
                  :max="maxStartMs"
                  step="1"
                  @input="clampStart"
                />
              </label>

              <label class="field compact">
                <span>End</span>
                <input
                  v-model.number="endMs"
                  name="trim_end_ms"
                  type="number"
                  :min="minEndMs"
                  :max="maxEndMs"
                  step="1"
                  @input="clampEnd"
                />
              </label>
            </div>

            <div v-if="validationMessage" class="trim-status warning">
              {{ validationMessage }}
            </div>
          </template>
        </div>

        <footer class="modal-footer">
          <button
            class="secondary-action ghost"
            type="button"
            :disabled="!trimInfo || saving"
            @click="resetRange"
          >
            <RotateCcw :size="16" />
            <span>Reset</span>
          </button>
          <div class="modal-footer-actions">
            <button class="secondary-action ghost" type="button" @click="emit('close')">
              Cancel
            </button>
            <button
              class="primary-action compact-action modal-save"
              type="button"
              :disabled="!canSave"
              @click="saveTrim"
            >
              <LoaderCircle v-if="saving" class="spin" :size="16" />
              <Save v-else :size="16" />
              <span>Save</span>
            </button>
          </div>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { LoaderCircle, Pause, Play, RotateCcw, Save, X } from 'lucide-vue-next';

import { applyAudioTrim } from '@/api/bridge';
import type { AudioState, AudioTrimInfoResponse, LineId, LineRecord } from '@/api/types';
import { useTrimRangeDrag } from '@/composables/useTrimRangeDrag';
import { loadAudioTrim } from '@/lib/audioTrimLoader';
import { createPcm16WavClipUrl, type DecodedPcm16Wav } from '@/lib/wavPreview';

const MIN_DURATION_MS = 100;
const ACTIVITY_BAR_COUNT = 192;

const props = defineProps<{
  line: LineRecord;
}>();

const emit = defineEmits<{
  close: [];
  saved: [payload: { lineId: LineId; audio: AudioState | null }];
}>();

const trimInfo = ref<AudioTrimInfoResponse | null>(null);
const startMs = ref(0);
const endMs = ref(0);
const loading = ref(true);
const saving = ref(false);
const playing = ref(false);
const error = ref<string | null>(null);
const activityBars = ref<number[]>(Array.from({ length: ACTIVITY_BAR_COUNT }, () => 0));
const playheadMs = ref(0);
const audioElement = ref<HTMLAudioElement | null>(null);
const activityTrack = ref<HTMLElement | null>(null);
const previewFrameId = ref<number | null>(null);
const decodedSource = ref<DecodedPcm16Wav | null>(null);
const previewUrl = ref<string | null>(null);

const selectedDurationMs = computed(() => Math.max(0, Math.round(endMs.value - startMs.value)));
const minStartMs = computed(() => 0);
const maxEndMs = computed(() => trimInfo.value?.sourceDurationMs ?? 0);
const maxStartMs = computed(() => Math.max(minStartMs.value, endMs.value - MIN_DURATION_MS));
const minEndMs = computed(() => Math.min(maxEndMs.value, startMs.value + MIN_DURATION_MS));
const sourceDurationMs = computed(() => trimInfo.value?.sourceDurationMs ?? 1);
const startPercent = computed(() => msToPercent(startMs.value));
const endPercent = computed(() => msToPercent(endMs.value));
const leftShadeStyle = computed(() => ({
  width: `${startPercent.value}%`,
}));
const rightShadeStyle = computed(() => ({
  left: `${endPercent.value}%`,
  width: `${Math.max(0, 100 - endPercent.value)}%`,
}));
const selectionStyle = computed(() => ({
  left: `${startPercent.value}%`,
  width: `${Math.max(0, endPercent.value - startPercent.value)}%`,
}));
const startHandleStyle = computed(() => ({
  left: `${startPercent.value}%`,
}));
const endHandleStyle = computed(() => ({
  left: `${endPercent.value}%`,
}));
const playheadStyle = computed(() => ({
  left: `${msToPercent(playheadMs.value)}%`,
}));
const validationMessage = computed(() => {
  const info = trimInfo.value;
  if (!info) {
    return '';
  }
  if (startMs.value >= endMs.value) {
    return 'Start must be before end.';
  }
  if (endMs.value > info.sourceDurationMs) {
    return 'End is outside the source audio.';
  }
  if (selectedDurationMs.value < MIN_DURATION_MS) {
    return `Minimum duration is ${MIN_DURATION_MS} ms.`;
  }
  return '';
});
const canSave = computed(() =>
  Boolean(trimInfo.value && !saving.value && !validationMessage.value),
);
const { beginHandleDrag, stopHandleDrag, suppressOverlayClose } = useTrimRangeDrag({
  track: activityTrack,
  sourceDurationMs,
  startMs,
  endMs,
  minStartMs,
  maxStartMs,
  minEndMs,
  maxEndMs,
});

onMounted(() => {
  void loadTrimInfo();
});

onBeforeUnmount(() => {
  pausePlayback();
  stopHandleDrag();
});

watch([startMs, endMs], () => {
  if (playing.value) {
    pausePlayback();
  }
  playheadMs.value = startMs.value;
});

async function loadTrimInfo(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const loaded = await loadAudioTrim(props.line.lineId, ACTIVITY_BAR_COUNT);
    trimInfo.value = loaded.info;
    startMs.value = loaded.info.startMs;
    endMs.value = loaded.info.endMs;
    decodedSource.value = loaded.decodedSource;
    activityBars.value = loaded.activityBars;
  } catch (loadError) {
    error.value = loadError instanceof Error ? loadError.message : 'Unable to load audio trim.';
  } finally {
    loading.value = false;
  }
}

async function togglePlayback(): Promise<void> {
  if (playing.value) {
    pausePlayback();
    return;
  }

  const element = audioElement.value;
  if (!element || validationMessage.value) {
    return;
  }

  const url = createPreviewUrl();
  replacePreviewUrl(url);
  element.src = url;
  element.currentTime = 0;
  playheadMs.value = startMs.value;
  try {
    await element.play();
    playing.value = true;
    startPreviewLoop();
  } catch (playError) {
    error.value = playError instanceof Error ? playError.message : 'Unable to play audio.';
  }
}

function handleTimeUpdate(): void {
  updatePreviewPosition();
}

function handlePlaybackEnded(): void {
  stopPreviewLoop();
  playing.value = false;
  playheadMs.value = endMs.value;
  clearPreviewUrl();
}

function startPreviewLoop(): void {
  stopPreviewLoop();
  previewFrameId.value = window.requestAnimationFrame(previewTick);
}

function previewTick(): void {
  updatePreviewPosition();
  if (playing.value) {
    previewFrameId.value = window.requestAnimationFrame(previewTick);
  }
}

function updatePreviewPosition(): void {
  const element = audioElement.value;
  if (!element || !playing.value) {
    return;
  }
  playheadMs.value = startMs.value + element.currentTime * 1_000;
  if (playheadMs.value >= endMs.value) {
    stopAtPreviewEnd();
  }
}

function pausePlayback(): void {
  if (audioElement.value) {
    audioElement.value.pause();
    audioElement.value.removeAttribute('src');
    audioElement.value.load();
  }
  stopPreviewLoop();
  clearPreviewUrl();
  playing.value = false;
}

function stopAtPreviewEnd(): void {
  const element = audioElement.value;
  if (element) {
    element.pause();
    element.removeAttribute('src');
    element.load();
  }
  stopPreviewLoop();
  clearPreviewUrl();
  playheadMs.value = endMs.value;
  playing.value = false;
}

function stopPreviewLoop(): void {
  if (previewFrameId.value != null) {
    window.cancelAnimationFrame(previewFrameId.value);
    previewFrameId.value = null;
  }
}

function resetRange(): void {
  const info = trimInfo.value;
  if (!info) {
    return;
  }
  startMs.value = info.startMs;
  endMs.value = info.endMs;
}

function clampStart(): void {
  startMs.value = clampNumber(startMs.value, minStartMs.value, maxStartMs.value);
}

function clampEnd(): void {
  endMs.value = clampNumber(endMs.value, minEndMs.value, maxEndMs.value);
}

function handleOverlayClick(): void {
  if (suppressOverlayClose.value) {
    return;
  }
  emit('close');
}

function createPreviewUrl(): string {
  const wav = decodedSource.value;
  if (!wav) {
    throw new Error('Audio preview is not ready yet.');
  }

  return createPcm16WavClipUrl(wav, startMs.value, endMs.value);
}

function replacePreviewUrl(url: string): void {
  clearPreviewUrl();
  previewUrl.value = url;
}

function clearPreviewUrl(): void {
  if (previewUrl.value) {
    URL.revokeObjectURL(previewUrl.value);
    previewUrl.value = null;
  }
}

function activityBarStyle(value: number): Record<string, string> {
  const height = Math.max(6, Math.round(value * 100));
  return {
    height: `${height}%`,
    opacity: String(0.35 + value * 0.65),
  };
}

function msToPercent(ms: number): number {
  return clampNumber((ms / sourceDurationMs.value) * 100, 0, 100);
}

async function saveTrim(): Promise<void> {
  const info = trimInfo.value;
  if (!info || validationMessage.value) {
    return;
  }

  saving.value = true;
  error.value = null;
  pausePlayback();
  try {
    const audio = await applyAudioTrim(props.line.lineId, {
      startMs: Math.round(startMs.value),
      endMs: Math.round(endMs.value),
    });
    emit('saved', { lineId: props.line.lineId, audio });
  } catch (saveError) {
    error.value = saveError instanceof Error ? saveError.message : 'Unable to save audio trim.';
  } finally {
    saving.value = false;
  }
}

function clampNumber(value: number, min: number, max: number): number {
  const parsed = Number.isFinite(value) ? value : min;
  return Math.min(Math.max(Math.round(parsed), min), max);
}
</script>
