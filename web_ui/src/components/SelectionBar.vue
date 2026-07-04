<template>
  <aside class="selection-bar">
    <div class="selection-summary">
      <span class="selection-count">{{ selectedCount }} selected</span>
      <span v-if="loadingTarget" class="target-preview">Loading target...</span>
      <span v-else-if="targetPreview" class="target-preview">{{ targetPreview }}</span>
      <span v-else-if="ankiConfigured" class="target-preview warning">No recent target</span>
    </div>

    <div class="selection-actions">
      <TooltipButton
        class="icon-button"
        type="button"
        tooltip="Mining settings"
        @click="emit('settings')"
      >
        <Settings :size="18" />
      </TooltipButton>
      <TooltipButton
        class="icon-button"
        type="button"
        :disabled="imageLoading"
        :tooltip="imageTooltip"
        @click="emit('requestImage')"
      >
        <LoaderCircle v-if="imageLoading" class="spin" :size="18" />
        <ImageIcon v-else :size="18" />
      </TooltipButton>
      <TooltipButton
        class="icon-button"
        type="button"
        :disabled="audioLoading"
        :tooltip="audioTooltip"
        @click="emit('requestAudio')"
      >
        <LoaderCircle v-if="audioLoading" class="spin" :size="18" />
        <Volume2 v-else :size="18" />
      </TooltipButton>
      <TooltipButton
        class="primary-action compact-action"
        type="button"
        :disabled="!canSend || sending"
        :tooltip="sendTooltip"
        @click="emit('sendToAnki')"
      >
        <LoaderCircle v-if="sending" class="spin" :size="18" />
        <Send v-else :size="18" />
        <span>{{ sending ? 'Sending' : 'Add to Anki' }}</span>
      </TooltipButton>
      <TooltipButton
        class="icon-button"
        type="button"
        tooltip="Clear selection"
        @click="emit('clear')"
      >
        <X :size="18" />
      </TooltipButton>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { Image as ImageIcon, LoaderCircle, Send, Settings, Volume2, X } from '@lucide/vue';

import TooltipButton from '@/components/TooltipButton.vue';

const props = defineProps<{
  selectedCount: number;
  targetPreview: string | null;
  loadingTarget: boolean;
  ankiConfigured: boolean;
  canSend: boolean;
  sending: boolean;
  imageLoading: boolean;
  audioLoading: boolean;
}>();

const emit = defineEmits<{
  settings: [];
  requestImage: [];
  requestAudio: [];
  sendToAnki: [];
  clear: [];
}>();

const imageTooltip = computed(() =>
  props.imageLoading ? 'Preparing selected screenshot' : 'Preview selected screenshot',
);
const audioTooltip = computed(() =>
  props.audioLoading ? 'Preparing selected audio' : 'Preview selected audio',
);
const sendTooltip = computed(() => {
  if (props.sending) {
    return 'Adding selected lines to Anki';
  }
  if (!props.ankiConfigured) {
    return 'Configure Anki fields first';
  }
  if (!props.canSend) {
    return 'No recent compatible Anki note';
  }
  return 'Add selected lines to Anki';
});
</script>
