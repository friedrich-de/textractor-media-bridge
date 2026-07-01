<template>
  <aside class="selection-bar" :class="{ inactive: selectedCount === 0 }">
    <div class="selection-summary">
      <template v-if="selectedCount > 0">
        <span class="selection-count">{{ selectedCount }} selected</span>
        <span v-if="loadingTarget" class="target-preview">Loading target...</span>
        <span v-else-if="targetPreview" class="target-preview">{{ targetPreview }}</span>
        <span v-else-if="ankiConfigured" class="target-preview warning">No recent target</span>
      </template>
      <span v-else>Click text rows to mine them.</span>
    </div>

    <div class="selection-actions">
      <button
        class="icon-button"
        type="button"
        aria-label="Mining settings"
        @click="emit('settings')"
      >
        <Settings :size="18" />
      </button>
      <button
        class="icon-button"
        type="button"
        :disabled="selectedCount === 0 || imageLoading"
        aria-label="Preview screenshot"
        @click="emit('requestImage')"
      >
        <LoaderCircle v-if="imageLoading" class="spin" :size="18" />
        <ImageIcon v-else :size="18" />
      </button>
      <button
        class="icon-button"
        type="button"
        :disabled="selectedCount === 0 || audioLoading"
        aria-label="Preview audio"
        @click="emit('requestAudio')"
      >
        <LoaderCircle v-if="audioLoading" class="spin" :size="18" />
        <Volume2 v-else :size="18" />
      </button>
      <button
        class="primary-action compact-action"
        type="button"
        :disabled="!canSend || sending"
        @click="emit('sendToAnki')"
      >
        <LoaderCircle v-if="sending" class="spin" :size="18" />
        <Send v-else :size="18" />
        <span>{{ sending ? 'Sending' : 'Add to Anki' }}</span>
      </button>
      <button
        class="icon-button"
        type="button"
        :disabled="selectedCount === 0"
        aria-label="Clear selection"
        @click="emit('clear')"
      >
        <X :size="18" />
      </button>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { Image as ImageIcon, LoaderCircle, Send, Settings, Volume2, X } from 'lucide-vue-next';

defineProps<{
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
</script>
