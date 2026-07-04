<template>
  <div ref="transcriptShell" class="transcript-shell">
    <div v-if="lines.length === 0" class="transcript-empty">
      <CaptionsOff :size="34" />
      <h2>No text lines</h2>
      <p>Keep Textractor attached to the selected text thread and leave this server running.</p>
    </div>

    <ol v-else class="transcript-list" aria-label="Textractor transcript">
      <li
        v-for="line in lines"
        :key="line.lineId"
        :ref="(element) => captureLine(element, line.lineId)"
        class="cue-row"
        :class="{
          active: line.lineId === activeLineId,
          past: line.lineSeq < activeLineSeq,
          selected: selectedLineIds.has(line.lineId),
        }"
        @click="emit('toggleLine', line.lineId)"
      >
        <div class="cue-content">
          <div class="cue-main">
            <div>
              <p>{{ line.text }}</p>
            </div>
            <div class="cue-actions">
              <TooltipButton
                class="icon-button small"
                type="button"
                tooltip="Copy text"
                @click.stop="emit('copyLine', line)"
              >
                <ClipboardCopy :size="16" />
              </TooltipButton>
              <TooltipButton
                class="icon-button small"
                type="button"
                :disabled="!line.screenshot"
                :tooltip="screenshotButtonTooltip(line)"
                @click.stop="emit('previewImage', line)"
              >
                <ImageIcon :size="16" />
              </TooltipButton>
              <TooltipButton
                class="icon-button small"
                type="button"
                :disabled="audioButtonDisabled(line)"
                :tooltip="audioButtonTooltip(line)"
                @click.stop="onAudioClick(line)"
              >
                <LoaderCircle v-if="line.audio?.status === 'recording'" class="spin" :size="16" />
                <Volume2 v-else :size="16" />
              </TooltipButton>
              <TooltipButton
                class="icon-button small"
                type="button"
                :disabled="trimButtonDisabled(line)"
                :tooltip="trimButtonTooltip(line)"
                @click.stop="onTrimClick(line)"
              >
                <LoaderCircle v-if="trimButtonRecording(line)" class="spin" :size="16" />
                <Scissors v-else :size="16" />
              </TooltipButton>
              <TooltipButton
                class="icon-button small"
                type="button"
                :disabled="removeAudioButtonDisabled(line)"
                :tooltip="removeAudioButtonTooltip(line)"
                @click.stop="emit('removeAudio', line)"
              >
                <VolumeX :size="16" />
              </TooltipButton>
              <TooltipButton
                class="icon-button small danger"
                type="button"
                tooltip="Delete line"
                @click.stop="emit('deleteLine', line)"
              >
                <Trash2 :size="16" />
              </TooltipButton>
            </div>
          </div>
        </div>
      </li>
    </ol>
  </div>
</template>

<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue';
import { computed, nextTick, shallowRef } from 'vue';
import {
  CaptionsOff,
  ClipboardCopy,
  Image as ImageIcon,
  LoaderCircle,
  Scissors,
  Trash2,
  Volume2,
  VolumeX,
} from '@lucide/vue';

import type { LineId, LineRecord } from '@/api/types';
import TooltipButton from '@/components/TooltipButton.vue';

const props = defineProps<{
  lines: readonly LineRecord[];
  activeLineId: LineId | null;
  selectedLineIds: ReadonlySet<LineId>;
}>();

const emit = defineEmits<{
  toggleLine: [lineId: LineId];
  copyLine: [line: LineRecord];
  previewImage: [line: LineRecord];
  previewAudio: [line: LineRecord];
  finishAudio: [line: LineRecord];
  trimAudio: [line: LineRecord];
  finishTrimAudio: [line: LineRecord];
  removeAudio: [line: LineRecord];
  deleteLine: [line: LineRecord];
}>();

const transcriptShell = shallowRef<HTMLElement | null>(null);
const lineElements = new Map<LineId, HTMLElement>();
const activeLineSeq = computed(
  () => props.lines.find((line) => line.lineId === props.activeLineId)?.lineSeq ?? 0,
);

function captureLine(element: Element | ComponentPublicInstance | null, lineId: LineId): void {
  if (element instanceof HTMLElement) {
    lineElements.set(lineId, element);
  } else {
    lineElements.delete(lineId);
  }
}

function onAudioClick(line: LineRecord): void {
  if (line.audio?.status === 'recording') {
    emit('finishAudio', line);
  } else {
    emit('previewAudio', line);
  }
}

function onTrimClick(line: LineRecord): void {
  if (trimButtonRecording(line)) {
    emit('finishTrimAudio', line);
  } else {
    emit('trimAudio', line);
  }
}

function audioButtonDisabled(line: LineRecord): boolean {
  return line.audio?.status !== 'recording' && line.audio?.status !== 'ready';
}

function audioButtonTooltip(line: LineRecord): string {
  if (line.audio?.status === 'recording') {
    return 'Stop recording audio';
  }
  if (line.audio?.status === 'ready') {
    return 'Replay recorded audio';
  }
  return 'No recorded audio';
}

function screenshotButtonTooltip(line: LineRecord): string {
  return line.screenshot ? 'Preview screenshot' : 'No screenshot available';
}

function trimButtonRecording(line: LineRecord): boolean {
  return (
    line.audio?.status === 'recording' ||
    (line.audio?.status === 'ready' && line.audio.trimRecordingStartedUnixMs != null)
  );
}

function trimButtonDisabled(line: LineRecord): boolean {
  return !trimButtonRecording(line) && line.audio?.status !== 'ready';
}

function trimButtonTooltip(line: LineRecord): string {
  if (trimButtonRecording(line)) {
    return 'Stop recording trimmable audio';
  }
  if (line.audio?.status === 'ready') {
    return 'Trim recorded audio';
  }
  return 'No trimmable audio';
}

function removeAudioButtonDisabled(line: LineRecord): boolean {
  return line.audio?.status !== 'recording' && line.audio?.status !== 'ready';
}

function removeAudioButtonTooltip(line: LineRecord): string {
  return removeAudioButtonDisabled(line) ? 'No audio to remove' : 'Remove audio from this line';
}

function scrollToTop(): void {
  transcriptShell.value?.scrollTo({ top: 0, behavior: 'smooth' });
  window.scrollTo({ top: 0, behavior: 'smooth' });
}

async function scrollToLine(lineId: LineId): Promise<void> {
  await nextTick();
  lineElements.get(lineId)?.scrollIntoView({
    block: 'center',
    behavior: 'smooth',
  });
}

defineExpose({
  scrollToTop,
  scrollToLine,
});
</script>
