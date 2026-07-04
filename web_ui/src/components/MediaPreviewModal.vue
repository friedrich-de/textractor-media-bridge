<template>
  <Teleport to="body">
    <div class="modal-overlay" @click.self="emit('close')">
      <section class="media-modal" role="dialog" aria-modal="true">
        <header class="modal-header">
          <div>
            <p>Image</p>
            <h2>Preview</h2>
          </div>
          <button
            class="icon-button"
            type="button"
            aria-label="Close preview"
            @click="emit('close')"
          >
            <X :size="18" />
          </button>
        </header>

        <div class="media-preview">
          <img :src="url" alt="" />
        </div>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { X } from '@lucide/vue';

defineProps<{
  url: string;
}>();

const emit = defineEmits<{
  close: [];
}>();
</script>
