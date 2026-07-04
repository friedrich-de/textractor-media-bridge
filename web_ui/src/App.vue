<template>
  <main class="app-shell">
    <nav class="floating-controls" aria-label="Reader controls">
      <button
        class="icon-button"
        type="button"
        aria-label="Scroll transcript to top"
        @click="scrollTranscriptTop"
      >
        <ArrowUp :size="18" />
      </button>
      <button
        class="icon-button"
        type="button"
        aria-label="Scroll to newest text"
        @click="scrollToNewest"
      >
        <LocateFixed :size="18" />
      </button>
      <button
        class="icon-button"
        type="button"
        aria-label="Mining settings"
        @click="showSettings = true"
      >
        <Settings :size="18" />
      </button>
    </nav>

    <ReaderView
      ref="readerView"
      :lines="visibleLines"
      :latest-line="latestVisibleLine"
      :selected-line-ids="selectedLineIds"
      :status="status"
      :loading="loading"
      :follow="follow"
      :character-count="characterCount"
      :clearable-line-count="lines.length"
      :clearing-lines="clearingLines"
      @reload="reloadLines"
      @clear-lines="clearLines"
      @toggle-follow="toggleFollow"
      @toggle-line="toggleLineSelection"
      @copy-line="copyLine"
      @preview-image="previewLineImage"
      @preview-audio="previewLineAudio"
      @finish-audio="finishAudio"
      @trim-audio="trimLineAudio"
      @finish-trim-audio="finishTrimAudio"
      @remove-audio="removeAudio"
    />

    <SelectionBar
      v-if="selectedLineCount > 0"
      :selected-count="selectedLineCount"
      :target-preview="targetCardPreview"
      :loading-target="loadingTargetCard"
      :anki-configured="ankiConfigured"
      :can-send="canSendToAnki"
      :sending="sendingToAnki"
      :image-loading="previewingMedia"
      :audio-loading="previewingMedia"
      @settings="showSettings = true"
      @request-image="previewSelectionImage"
      @request-audio="previewSelectionAudio"
      @send-to-anki="sendSelectionToAnki"
      @clear="clearSelection"
    />

    <SettingsModal
      v-if="showSettings"
      :settings="settings"
      :audio-config="config?.config.audio ?? null"
      :line-config="config?.config.lines ?? null"
      @save="saveSettings"
      @cancel="showSettings = false"
    />

    <MediaPreviewModal v-if="mediaPreview" :url="mediaPreview.url" @close="mediaPreview = null" />

    <AudioTrimModal
      v-if="audioTrimLine"
      :line="audioTrimLine"
      @close="audioTrimLine = null"
      @saved="handleAudioTrimSaved"
    />

    <ToastStack :toasts="toasts" @dismiss="dismissToast" />
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ArrowUp, LocateFixed, Settings } from '@lucide/vue';

import { assetUrl, updateConfig } from '@/api/bridge';
import type { AudioConfig, AudioState, LineConfig, LineId, LineRecord } from '@/api/types';
import AudioTrimModal from '@/components/AudioTrimModal.vue';
import MediaPreviewModal from '@/components/MediaPreviewModal.vue';
import ReaderView from '@/components/ReaderView.vue';
import SelectionBar from '@/components/SelectionBar.vue';
import SettingsModal from '@/components/SettingsModal.vue';
import ToastStack from '@/components/ToastStack.vue';
import { useAnkiMining } from '@/composables/useAnkiMining';
import { useBridgeLines } from '@/composables/useBridgeLines';
import { useMiningSelection } from '@/composables/useMiningSelection';
import { useToast } from '@/composables/useToast';
import {
  cloneMinerSettings,
  loadMinerSettings,
  saveMinerSettings,
  type MinerSettings,
} from '@/lib/minerSettings';
import { filterLineRecords } from '@/lib/textFilters';

type ReaderViewHandle = {
  scrollToTop: () => void;
  scrollToLatest: () => void;
};

type SettingsSavePayload = {
  settings: MinerSettings;
  audioConfig: AudioConfig;
  lineConfig: LineConfig;
};

const { toasts, toast, dismissToast } = useToast();
const settings = ref<MinerSettings>(loadMinerSettings());
const showSettings = ref(false);
const follow = ref(true);
const readerView = ref<ReaderViewHandle | null>(null);
const mediaPreview = ref<{ url: string } | null>(null);
const audioPreview = ref<HTMLAudioElement | null>(null);
const audioTrimLine = ref<LineRecord | null>(null);
const clearingLines = ref(false);

const {
  config,
  lines,
  loading,
  status,
  start,
  reloadLines,
  clearLines: clearServerLines,
  finishLineAudio,
  finishLineTrimAudio,
  removeLineAudio,
  updateLineAudio,
} = useBridgeLines(toast);

const visibleLines = computed(() => filterLineRecords(lines.value, settings.value.textFilters));
const latestVisibleLine = computed(() => visibleLines.value.at(-1) ?? null);
const currentLines = computed(() => visibleLines.value);
const characterCount = computed(() =>
  visibleLines.value.reduce((total, line) => total + Array.from(line.text).length, 0),
);
const {
  selectedLineIds,
  selectedLineCount,
  selectedLines,
  toggleLineSelection,
  clearSelection,
  pruneSelection,
} = useMiningSelection(currentLines);

const {
  targetCardPreview,
  loadingTargetCard,
  sendingToAnki,
  previewingMedia,
  ankiConfigured,
  canSendToAnki,
  previewSelectionImage: getSelectionImage,
  previewSelectionAudio: getSelectionAudio,
  sendSelectionToAnki,
  resetTargetPreview,
} = useAnkiMining({
  settings,
  selectedLines,
  selectedLineCount,
  clearSelection,
  toast,
});

onMounted(async () => {
  try {
    await start();
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to start bridge UI.');
  }
});

watch(visibleLines, (nextLines) => {
  pruneSelection(new Set(nextLines.map((line) => line.lineId)));
});

async function copyLine(line: LineRecord): Promise<void> {
  await navigator.clipboard?.writeText(line.text);
  toast.success('Copied text.');
}

function previewLineImage(line: LineRecord): void {
  if (!line.screenshot) {
    toast.warning('No screenshot for this line.');
    return;
  }
  mediaPreview.value = { url: assetUrl(line.screenshot.url) };
}

async function previewLineAudio(line: LineRecord): Promise<void> {
  if (line.audio?.status !== 'ready') {
    toast.warning('No audio for this line.');
    return;
  }
  await playAudioPreview(assetUrl(line.audio.asset.url));
}

function scrollTranscriptTop(): void {
  follow.value = false;
  readerView.value?.scrollToTop();
}

function scrollToNewest(): void {
  follow.value = true;
  readerView.value?.scrollToLatest();
}

function toggleFollow(): void {
  follow.value = !follow.value;
  if (follow.value) {
    readerView.value?.scrollToLatest();
  }
}

async function clearLines(): Promise<void> {
  if (lines.value.length === 0 || clearingLines.value) {
    return;
  }

  clearingLines.value = true;
  try {
    await clearServerLines();
    clearSelection();
    mediaPreview.value = null;
    audioTrimLine.value = null;
    resetTargetPreview();
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to clear lines.');
  } finally {
    clearingLines.value = false;
  }
}

async function finishAudio(line: LineRecord): Promise<void> {
  try {
    await finishLineAudio(line.lineId);
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to finish audio.');
  }
}

function trimLineAudio(line: LineRecord): void {
  if (line.audio?.status !== 'ready') {
    toast.warning('No audio for this line.');
    return;
  }
  if (line.audio.trimRecordingStartedUnixMs != null) {
    toast.warning('Trim audio is still recording.');
    return;
  }
  audioTrimLine.value = line;
}

async function finishTrimAudio(line: LineRecord): Promise<void> {
  try {
    await finishLineTrimAudio(line.lineId);
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to finish trim audio.');
  }
}

async function removeAudio(line: LineRecord): Promise<void> {
  try {
    await removeLineAudio(line.lineId);
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to remove audio.');
  }
}

function handleAudioTrimSaved(payload: { lineId: LineId; audio: AudioState | null }): void {
  updateLineAudio(payload.lineId, payload.audio);
  audioTrimLine.value = null;
  toast.success('Audio trim saved.');
}

async function previewSelectionImage(): Promise<void> {
  try {
    const url = await getSelectionImage();
    if (!url) {
      toast.warning('No screenshot is available for the selection.');
      return;
    }
    mediaPreview.value = { url: assetUrl(url) };
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to prepare screenshot.');
  }
}

async function previewSelectionAudio(): Promise<void> {
  try {
    const url = await getSelectionAudio();
    if (!url) {
      toast.warning('No audio is available for the selection.');
      return;
    }
    await playAudioPreview(assetUrl(url));
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to prepare audio.');
  }
}

async function playAudioPreview(url: string): Promise<void> {
  audioPreview.value?.pause();
  audioPreview.value = new Audio(url);
  audioPreview.value.preload = 'auto';

  try {
    await audioPreview.value.play();
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to play audio.');
  }
}

async function saveSettings(payload: SettingsSavePayload): Promise<void> {
  settings.value = cloneMinerSettings(payload.settings);
  saveMinerSettings(settings.value);

  try {
    config.value = await updateConfig({
      audio: payload.audioConfig,
      lines: payload.lineConfig,
    });
    resetTargetPreview();
    showSettings.value = false;
    toast.success('Settings saved.');
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to save audio settings.');
  }
}
</script>
