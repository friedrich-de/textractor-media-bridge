import { ref, type Ref } from 'vue';

export type TrimDragHandle = 'start' | 'end';

type ReadonlyRef<T> = {
  readonly value: T;
};

interface UseTrimRangeDragOptions {
  track: ReadonlyRef<HTMLElement | null>;
  sourceDurationMs: ReadonlyRef<number>;
  startMs: Ref<number>;
  endMs: Ref<number>;
  minStartMs: ReadonlyRef<number>;
  maxStartMs: ReadonlyRef<number>;
  minEndMs: ReadonlyRef<number>;
  maxEndMs: ReadonlyRef<number>;
}

type DragState = {
  handle: TrimDragHandle;
  left: number;
  width: number;
};

export function useTrimRangeDrag(options: UseTrimRangeDragOptions): {
  suppressOverlayClose: Ref<boolean>;
  beginHandleDrag: (handle: TrimDragHandle, event: PointerEvent) => void;
  stopHandleDrag: () => void;
} {
  const dragState = ref<DragState | null>(null);
  const suppressOverlayClose = ref(false);

  function beginHandleDrag(handle: TrimDragHandle, event: PointerEvent): void {
    const track = options.track.value;
    if (!track) {
      return;
    }

    (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
    const rect = track.getBoundingClientRect();
    dragState.value = {
      handle,
      left: rect.left,
      width: rect.width,
    };
    suppressOverlayClose.value = true;
    window.addEventListener('pointermove', handleDragMove);
    window.addEventListener('pointerup', stopHandleDrag, { once: true });
    handleDragMove(event);
  }

  function handleDragMove(event: PointerEvent): void {
    const drag = dragState.value;
    const durationMs = options.sourceDurationMs.value;
    if (!drag || durationMs <= 0 || drag.width <= 0) {
      return;
    }

    const ratio = (event.clientX - drag.left) / drag.width;
    const pointerMs = clampNumber(ratio * durationMs, 0, durationMs);
    if (drag.handle === 'start') {
      options.startMs.value = clampNumber(
        pointerMs,
        options.minStartMs.value,
        options.maxStartMs.value,
      );
      return;
    }

    options.endMs.value = clampNumber(pointerMs, options.minEndMs.value, options.maxEndMs.value);
  }

  function stopHandleDrag(): void {
    dragState.value = null;
    window.removeEventListener('pointermove', handleDragMove);
    window.setTimeout(() => {
      suppressOverlayClose.value = false;
    }, 150);
  }

  return {
    suppressOverlayClose,
    beginHandleDrag,
    stopHandleDrag,
  };
}

function clampNumber(value: number, min: number, max: number): number {
  const parsed = Number.isFinite(value) ? value : min;
  return Math.min(Math.max(Math.round(parsed), min), max);
}
