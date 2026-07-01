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
      :lines="lines"
      :latest-line="latestLine"
      :selected-line-ids="selectedLineIds"
      :status="status"
      :loading="loading"
      :follow="follow"
      @reload="reloadLines"
      @toggle-follow="toggleFollow"
      @toggle-line="toggleLineSelection"
      @copy-line="copyLine"
      @preview-image="previewLineImage"
      @preview-audio="previewLineAudio"
      @finish-audio="finishAudio"
    />

    <SelectionBar
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
      @save="saveSettings"
      @cancel="showSettings = false"
    />

    <MediaPreviewModal v-if="mediaPreview" :url="mediaPreview.url" @close="mediaPreview = null" />

    <ToastStack :toasts="toasts" @dismiss="dismissToast" />
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { ArrowUp, LocateFixed, Settings } from 'lucide-vue-next';

import { assetUrl } from '@/api/bridge';
import type { LineRecord } from '@/api/types';
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

type ReaderViewHandle = {
  scrollToTop: () => void;
  scrollToLatest: () => void;
};

const { toasts, toast, dismissToast } = useToast();
const settings = ref<MinerSettings>(loadMinerSettings());
const showSettings = ref(false);
const follow = ref(true);
const readerView = ref<ReaderViewHandle | null>(null);
const mediaPreview = ref<{ url: string } | null>(null);
const audioPreview = ref<HTMLAudioElement | null>(null);

const { token, lines, loading, status, latestLine, start, reloadLines, finishLineAudio } =
  useBridgeLines(toast);

const currentLines = computed(() => lines.value);
const { selectedLineIds, selectedLineCount, selectedLines, toggleLineSelection, clearSelection } =
  useMiningSelection(currentLines);

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
  token,
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

async function copyLine(line: LineRecord): Promise<void> {
  await navigator.clipboard?.writeText(line.text);
  toast.success('Copied text.');
}

function previewLineImage(line: LineRecord): void {
  if (!line.screenshot) {
    toast.warning('No screenshot for this line.');
    return;
  }
  mediaPreview.value = { url: assetUrl(line.screenshot.url, token.value) };
}

async function previewLineAudio(line: LineRecord): Promise<void> {
  if (line.audio?.status !== 'ready') {
    toast.warning('No audio for this line.');
    return;
  }
  await playAudioPreview(assetUrl(line.audio.asset.url, token.value));
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

async function finishAudio(line: LineRecord): Promise<void> {
  try {
    await finishLineAudio(line.lineId);
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Unable to finish audio.');
  }
}

async function previewSelectionImage(): Promise<void> {
  try {
    const url = await getSelectionImage();
    if (!url) {
      toast.warning('No screenshot is available for the selection.');
      return;
    }
    mediaPreview.value = { url: assetUrl(url, token.value) };
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
    await playAudioPreview(assetUrl(url, token.value));
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

function saveSettings(nextSettings: MinerSettings): void {
  settings.value = cloneMinerSettings(nextSettings);
  saveMinerSettings(settings.value);
  resetTargetPreview();
  showSettings.value = false;
  toast.success('Mining settings saved.');
}
</script>
