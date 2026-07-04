<template>
  <section class="reader-view">
    <header class="reader-toolbar" :class="{ 'without-title': !gameTitle }">
      <div v-if="gameTitle" class="game-title">
        <Gamepad2 :size="18" />
        <span>{{ gameTitle }}</span>
      </div>

      <div class="toolbar-cluster">
        <TooltipButton
          class="secondary-action track-toggle"
          :class="{ active: follow }"
          type="button"
          :aria-pressed="follow"
          :tooltip="follow ? 'Stop following latest line' : 'Follow latest line'"
          @click="emit('toggleFollow')"
        >
          <LocateFixed :size="18" />
          <span>{{ follow ? 'Tracking' : 'Track' }}</span>
        </TooltipButton>
        <TooltipButton
          class="time-pill"
          type="button"
          tooltip="Jump to latest line"
          @click="scrollToLatest"
        >
          <Clock3 :size="16" />
          <span>{{ latestLine ? formatTime(latestLine.timestampUnixMs) : '--:--' }}</span>
          <span>/ {{ lines.length }} lines</span>
        </TooltipButton>
        <span class="stat-pill">{{ formattedCharacterCount }} chars</span>
        <TooltipButton
          class="secondary-action danger compact-text-action"
          type="button"
          :disabled="clearableLineCount === 0 || clearingLines"
          :tooltip="clearingLines ? 'Clearing transcript' : 'Clear transcript lines'"
          @click="emit('clearLines')"
        >
          <LoaderCircle v-if="clearingLines" class="spin" :size="18" />
          <Trash2 v-else :size="18" />
          <span>Clear</span>
        </TooltipButton>
        <TooltipButton
          class="icon-button"
          type="button"
          tooltip="Reload lines"
          @click="emit('reload')"
        >
          <RefreshCw :size="18" />
        </TooltipButton>
      </div>
    </header>

    <LineTranscript
      ref="transcript"
      :lines="lines"
      :active-line-id="latestLine?.lineId ?? null"
      :selected-line-ids="selectedLineIds"
      @toggle-line="emit('toggleLine', $event)"
      @copy-line="emit('copyLine', $event)"
      @preview-image="emit('previewImage', $event)"
      @preview-audio="emit('previewAudio', $event)"
      @finish-audio="emit('finishAudio', $event)"
      @trim-audio="emit('trimAudio', $event)"
      @finish-trim-audio="emit('finishTrimAudio', $event)"
      @remove-audio="emit('removeAudio', $event)"
    />
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Clock3, Gamepad2, LoaderCircle, LocateFixed, RefreshCw, Trash2 } from '@lucide/vue';

import type { LiveStatus } from '@/composables/useBridgeLines';
import type { LineId, LineRecord } from '@/api/types';
import LineTranscript from '@/components/LineTranscript.vue';
import TooltipButton from '@/components/TooltipButton.vue';

type TranscriptHandle = {
  scrollToTop: () => void;
  scrollToLine: (lineId: LineId) => Promise<void>;
};

const props = defineProps<{
  lines: readonly LineRecord[];
  latestLine: LineRecord | null;
  selectedLineIds: ReadonlySet<LineId>;
  status: LiveStatus;
  loading: boolean;
  follow: boolean;
  characterCount: number;
  clearableLineCount: number;
  clearingLines: boolean;
}>();

const emit = defineEmits<{
  reload: [];
  clearLines: [];
  toggleFollow: [];
  toggleLine: [lineId: LineId];
  copyLine: [line: LineRecord];
  previewImage: [line: LineRecord];
  previewAudio: [line: LineRecord];
  finishAudio: [line: LineRecord];
  trimAudio: [line: LineRecord];
  finishTrimAudio: [line: LineRecord];
  removeAudio: [line: LineRecord];
}>();

const transcript = ref<TranscriptHandle | null>(null);

const gameTitle = computed(() => props.latestLine?.meta.windowTitle?.trim() ?? '');
const formattedCharacterCount = computed(() =>
  new Intl.NumberFormat().format(props.characterCount),
);

watch(
  () => [props.latestLine?.lineId, props.follow] as const,
  ([lineId, shouldFollow]) => {
    if (shouldFollow && lineId != null) {
      void transcript.value?.scrollToLine(lineId);
    }
  },
);

function scrollToTop(): void {
  transcript.value?.scrollToTop();
}

function scrollToLatest(): void {
  if (props.latestLine) {
    void transcript.value?.scrollToLine(props.latestLine.lineId);
  }
}

function formatTime(unixMs: number): string {
  return new Date(unixMs).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

defineExpose({
  scrollToTop,
  scrollToLatest,
});
</script>
