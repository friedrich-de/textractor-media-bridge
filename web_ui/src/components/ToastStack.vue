<template>
  <div class="toast-stack" aria-live="polite">
    <div v-for="toast in toasts" :key="toast.id" class="toast" :data-type="toast.type">
      <span>{{ toast.message }}</span>
      <button v-if="toast.action" type="button" @click="toast.action.onClick">
        {{ toast.action.label }}
      </button>
      <button type="button" aria-label="Dismiss" @click="emit('dismiss', toast.id)">
        <X :size="15" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next';

import type { Toast } from '@/composables/useToast';

defineProps<{
  toasts: readonly Toast[];
}>();

const emit = defineEmits<{
  dismiss: [id: number];
}>();
</script>
