<template>
  <section class="reader-view">
    <header class="reader-toolbar" :class="{ 'without-title': !gameTitle }">
      <div v-if="gameTitle" class="game-title">
        <Gamepad2 :size="18" />
        <span>{{ gameTitle }}</span>
      </div>

      <div class="toolbar-cluster">
        <button
          class="secondary-action track-toggle"
          :class="{ active: follow }"
          type="button"
          :aria-pressed="follow"
          :aria-label="follow ? 'Disable line tracking' : 'Enable line tracking'"
          @click="emit('toggleFollow')"
        >
          <LocateFixed :size="18" />
          <span>{{ follow ? 'Tracking' : 'Track' }}</span>
        </button>
        <button
          class="time-pill"
          type="button"
          aria-label="Scroll to latest text line"
          @click="scrollToLatest"
        >
          <Clock3 :size="16" />
          <span>{{ latestLine ? formatTime(latestLine.timestampUnixMs) : '--:--' }}</span>
          <span>/ {{ lines.length }} lines</span>
        </button>
        <button class="icon-button" type="button" aria-label="Reload lines" @click="emit('reload')">
          <RefreshCw :size="18" />
        </button>
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
    />
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Clock3, Gamepad2, LocateFixed, RefreshCw } from 'lucide-vue-next';

import type { LiveStatus } from '@/composables/useBridgeLines';
import type { LineId, LineRecord } from '@/api/types';
import LineTranscript from '@/components/LineTranscript.vue';

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
}>();

const emit = defineEmits<{
  reload: [];
  toggleFollow: [];
  toggleLine: [lineId: LineId];
  copyLine: [line: LineRecord];
  previewImage: [line: LineRecord];
  previewAudio: [line: LineRecord];
  finishAudio: [line: LineRecord];
  trimAudio: [line: LineRecord];
  finishTrimAudio: [line: LineRecord];
}>();

const transcript = ref<TranscriptHandle | null>(null);

const gameTitle = computed(() => props.latestLine?.meta.windowTitle?.trim() ?? '');

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
