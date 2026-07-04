<template>
  <div class="toast-stack" aria-live="polite">
    <div v-for="toast in toasts" :key="toast.id" class="toast" :data-type="toast.type">
      <span>{{ toast.message }}</span>
      <button v-if="toast.action" type="button" @click="toast.action.onClick">
        {{ toast.action.label }}
      </button>
      <TooltipButton
        type="button"
        tooltip="Dismiss notification"
        @click="emit('dismiss', toast.id)"
      >
        <X :size="15" />
      </TooltipButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { X } from '@lucide/vue';

import type { Toast } from '@/composables/useToast';
import TooltipButton from '@/components/TooltipButton.vue';

defineProps<{
  toasts: readonly Toast[];
}>();

const emit = defineEmits<{
  dismiss: [id: number];
}>();
</script>
