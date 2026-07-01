import { computed, onBeforeUnmount, ref } from 'vue';

import {
  finishAudio,
  finishTrimAudio,
  getConfig,
  getLines,
  loadSessionToken,
  openEventSource,
  parseBrowserEvent,
} from '@/api/bridge';
import type { AudioState, LineId, LinePatch, LineRecord, PublicConfig } from '@/api/types';
import type { ToastApi } from '@/composables/useToast';

export type LiveStatus = 'starting' | 'loading' | 'live' | 'reconnecting' | 'error';

export function useBridgeLines(toast: ToastApi) {
  const token = ref(loadSessionToken());
  const config = ref<PublicConfig | null>(null);
  const lines = ref<LineRecord[]>([]);
  const loading = ref(false);
  const status = ref<LiveStatus>('starting');
  const newestSeq = ref(0);
  const oldestSeq = ref<number | null>(null);
  const hasMoreOlder = ref(false);
  let eventSource: EventSource | null = null;

  const latestLine = computed(() => lines.value.at(-1) ?? null);

  async function start(): Promise<void> {
    status.value = 'loading';
    config.value = await getConfig(token.value);
    if (config.value.sessionToken) {
      token.value = config.value.sessionToken;
    }
    await reloadLines();
    connectEvents();
  }

  async function reloadLines(): Promise<void> {
    loading.value = true;
    try {
      const page = await getLines(token.value, { limit: 180 });
      lines.value = normalizeLines(page.lines);
      newestSeq.value = page.newestSeq ?? newestSeq.value;
      oldestSeq.value = page.oldestSeq ?? null;
      hasMoreOlder.value = page.hasMoreOlder;
    } finally {
      loading.value = false;
    }
  }

  async function loadOlder(): Promise<void> {
    if (!hasMoreOlder.value || loading.value || oldestSeq.value == null) {
      return;
    }

    loading.value = true;
    try {
      const page = await getLines(token.value, {
        limit: 100,
        beforeSeq: oldestSeq.value,
      });
      lines.value = normalizeLines([...page.lines, ...lines.value]);
      oldestSeq.value = page.oldestSeq ?? oldestSeq.value;
      hasMoreOlder.value = page.hasMoreOlder;
    } finally {
      loading.value = false;
    }
  }

  async function finishLineAudio(lineId: LineId): Promise<void> {
    const audio = await finishAudio(token.value, lineId);
    patchLine(lineId, { audio });
  }

  async function finishLineTrimAudio(lineId: LineId): Promise<void> {
    const audio = await finishTrimAudio(token.value, lineId);
    patchLine(lineId, { audio });
  }

  function updateLineAudio(lineId: LineId, audio: AudioState | null): void {
    patchLine(lineId, { audio });
  }

  function connectEvents(): void {
    eventSource?.close();
    eventSource = openEventSource(token.value);

    eventSource.addEventListener('open', () => {
      status.value = 'live';
    });
    eventSource.addEventListener('hello', (event) => {
      const payload = parseBrowserEvent(event);
      if (payload.type === 'hello' && payload.newestSeq) {
        newestSeq.value = Math.max(newestSeq.value, payload.newestSeq);
      }
    });
    eventSource.addEventListener('line_added', (event) => {
      const payload = parseBrowserEvent(event);
      if (payload.type !== 'line_added') {
        return;
      }
      upsertLine(payload.line);
      newestSeq.value = Math.max(newestSeq.value, payload.line.lineSeq);
    });
    eventSource.addEventListener('line_updated', (event) => {
      const payload = parseBrowserEvent(event);
      if (payload.type !== 'line_updated') {
        return;
      }
      patchLine(payload.lineId, payload.patch);
    });
    eventSource.addEventListener('error', () => {
      status.value = 'reconnecting';
      window.setTimeout(() => {
        void syncAfterNewest();
      }, 1_600);
    });
  }

  async function syncAfterNewest(): Promise<void> {
    if (!newestSeq.value) {
      return;
    }

    try {
      const page = await getLines(token.value, {
        limit: 500,
        afterSeq: newestSeq.value,
      });
      page.lines.forEach(upsertLine);
      if (page.newestSeq) {
        newestSeq.value = Math.max(newestSeq.value, page.newestSeq);
      }
    } catch (error) {
      toast.warning(error instanceof Error ? error.message : 'Unable to sync missed lines.');
    }
  }

  function upsertLine(line: LineRecord): void {
    const next = lines.value.filter((item) => item.lineId !== line.lineId);
    next.push(line);
    lines.value = normalizeLines(next);
  }

  function patchLine(lineId: LineId, patch: LinePatch): void {
    lines.value = lines.value.map((line) => {
      if (line.lineId !== lineId) {
        return line;
      }

      return {
        ...line,
        screenshot: 'screenshot' in patch ? patch.screenshot : line.screenshot,
        audio: 'audio' in patch ? patch.audio : line.audio,
        warnings: patch.warnings ?? line.warnings,
        ignored: patch.ignored ?? line.ignored,
      };
    });
  }

  onBeforeUnmount(() => {
    eventSource?.close();
  });

  return {
    token,
    config,
    lines,
    loading,
    status,
    latestLine,
    hasMoreOlder,
    start,
    reloadLines,
    loadOlder,
    finishLineAudio,
    finishLineTrimAudio,
    updateLineAudio,
  };
}

function normalizeLines(lines: LineRecord[]): LineRecord[] {
  const byId = new Map<LineId, LineRecord>();
  for (const line of lines) {
    byId.set(line.lineId, line);
  }
  return [...byId.values()].sort((a, b) => a.lineSeq - b.lineSeq);
}
