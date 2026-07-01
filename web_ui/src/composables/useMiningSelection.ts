import { computed, ref, type ComputedRef } from 'vue';

import type { LineId, LineRecord } from '@/api/types';

export interface SelectedRange {
  firstIndex: number;
  lastIndex: number;
  firstLineId: LineId;
  lastLineId: LineId;
}

export function useMiningSelection(lines: ComputedRef<readonly LineRecord[]>) {
  const selectedLineIds = ref<Set<LineId>>(new Set());
  const selectedLineIdList = computed(() => [...selectedLineIds.value]);
  const selectedLineCount = computed(() => selectedLineIds.value.size);
  const selectedLines = computed(() =>
    lines.value.filter((line) => selectedLineIds.value.has(line.lineId)),
  );
  const selectedRange = computed(() => getSelectionBounds(lines.value, selectedLineIds.value));

  function toggleLineSelection(lineId: LineId): void {
    selectedLineIds.value = toggleContiguousSelection(lines.value, selectedLineIds.value, lineId);
  }

  function clearSelection(): void {
    selectedLineIds.value = new Set();
  }

  return {
    selectedLineIds,
    selectedLineIdList,
    selectedLineCount,
    selectedLines,
    selectedRange,
    toggleLineSelection,
    clearSelection,
  };
}

function toggleContiguousSelection(
  lines: readonly LineRecord[],
  selectedIds: ReadonlySet<LineId>,
  lineId: LineId,
): Set<LineId> {
  const next = new Set(selectedIds);
  const clickedIndex = lines.findIndex((line) => line.lineId === lineId);
  if (clickedIndex === -1) {
    return next;
  }

  const bounds = getSelectionBounds(lines, next);
  if (next.has(lineId)) {
    if (!bounds || clickedIndex === bounds.firstIndex || clickedIndex === bounds.lastIndex) {
      next.delete(lineId);
    }
  } else if (
    next.size === 0 ||
    !bounds ||
    clickedIndex === bounds.firstIndex - 1 ||
    clickedIndex === bounds.lastIndex + 1
  ) {
    next.add(lineId);
  }

  return next;
}

function getSelectionBounds(
  lines: readonly LineRecord[],
  selectedIds: ReadonlySet<LineId>,
): SelectedRange | null {
  let firstIndex = Number.POSITIVE_INFINITY;
  let lastIndex = Number.NEGATIVE_INFINITY;

  lines.forEach((line, index) => {
    if (!selectedIds.has(line.lineId)) {
      return;
    }
    firstIndex = Math.min(firstIndex, index);
    lastIndex = Math.max(lastIndex, index);
  });

  if (selectedIds.size === 0 || !Number.isFinite(firstIndex) || !Number.isFinite(lastIndex)) {
    return null;
  }

  return {
    firstIndex,
    lastIndex,
    firstLineId: lines[firstIndex].lineId,
    lastLineId: lines[lastIndex].lineId,
  };
}
