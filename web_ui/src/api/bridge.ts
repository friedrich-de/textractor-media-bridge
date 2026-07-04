import type {
  AssetBase64Response,
  AudioState,
  AudioTrimInfoResponse,
  AudioTrimRequest,
  BrowserEventPayload,
  EditableConfigRequest,
  LineHistoryPage,
  LineId,
  MinePrepareRequest,
  MinePrepareResponse,
  PublicConfig,
} from '@/api/types';

export class BridgeApiError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'BridgeApiError';
  }
}

export function assetUrl(url: string): string {
  return url;
}

export async function getConfig(): Promise<PublicConfig> {
  return apiJson<PublicConfig>('/api/config');
}

export async function updateConfig(config: EditableConfigRequest): Promise<PublicConfig> {
  return apiJson<PublicConfig>('/api/config', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export async function getLines(
  query: { limit?: number; beforeSeq?: number; afterSeq?: number } = {},
): Promise<LineHistoryPage> {
  const params = new URLSearchParams();
  if (query.limit != null) {
    params.set('limit', String(query.limit));
  }
  if (query.beforeSeq != null) {
    params.set('beforeSeq', String(query.beforeSeq));
  }
  if (query.afterSeq != null) {
    params.set('afterSeq', String(query.afterSeq));
  }

  const suffix = params.toString() ? `?${params}` : '';
  return apiJson<LineHistoryPage>(`/api/lines${suffix}`);
}

export async function clearLines(): Promise<{ clearedLines: number }> {
  return apiJson<{ clearedLines: number }>('/api/lines', { method: 'DELETE' });
}

export async function finishAudio(lineId: LineId): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/finish`,
    { method: 'POST' },
  );
  return response.audio ?? null;
}

export async function removeAudio(lineId: LineId): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio`,
    { method: 'DELETE' },
  );
  return response.audio ?? null;
}

export async function getAudioTrimInfo(lineId: LineId): Promise<AudioTrimInfoResponse> {
  return apiJson<AudioTrimInfoResponse>(`/api/lines/${lineId}/audio/trim`);
}

export async function applyAudioTrim(
  lineId: LineId,
  request: AudioTrimRequest,
): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/trim`,
    {
      method: 'POST',
      body: JSON.stringify(request),
    },
  );
  return response.audio ?? null;
}

export async function finishTrimAudio(lineId: LineId): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/trim/finish`,
    { method: 'POST' },
  );
  return response.audio ?? null;
}

export async function prepareMine(request: MinePrepareRequest): Promise<MinePrepareResponse> {
  return apiJson<MinePrepareResponse>('/api/mine/prepare', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

export async function getAssetBase64(assetId: string): Promise<AssetBase64Response> {
  return apiJson<AssetBase64Response>(`/api/assets/${assetId}/base64`, {
    method: 'POST',
  });
}

export function openEventSource(): EventSource {
  return new EventSource('/api/events');
}

export function parseBrowserEvent(event: MessageEvent<string>): BrowserEventPayload {
  return JSON.parse(event.data) as BrowserEventPayload;
}

async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      ...init,
      headers: {
        'Content-Type': 'application/json',
        ...init.headers,
      },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new BridgeApiError(`Network error: ${message}`);
  }

  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const data = (await response.json()) as { message?: string };
      message = data.message ?? message;
    } catch {
      // Keep HTTP fallback message.
    }
    throw new BridgeApiError(message);
  }

  return (await response.json()) as T;
}
