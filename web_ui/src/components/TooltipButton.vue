<template>
  <span
    class="tooltip-button"
    :data-tooltip="tooltip"
    :tabindex="disabled ? 0 : undefined"
    :aria-label="disabled ? accessibleLabel : undefined"
    :aria-disabled="disabled ? 'true' : undefined"
  >
    <button v-bind="attrs" :disabled="disabled" :aria-label="accessibleLabel" @click="handleClick">
      <slot />
    </button>
  </span>
</template>

<script setup lang="ts">
import { computed, useAttrs } from 'vue';

defineOptions({
  inheritAttrs: false,
});

const props = withDefaults(
  defineProps<{
    tooltip: string;
    label?: string;
    disabled?: boolean;
  }>(),
  {
    label: undefined,
    disabled: false,
  },
);

const attrs = useAttrs();
const emit = defineEmits<{
  click: [event: MouseEvent];
}>();
const accessibleLabel = computed(() => props.label ?? props.tooltip);

function handleClick(event: MouseEvent): void {
  if (props.disabled) {
    return;
  }
  emit('click', event);
}
</script>
