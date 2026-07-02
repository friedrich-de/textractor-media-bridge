import type {
  AssetBase64Response,
  AudioConfig,
  AudioState,
  AudioTrimInfoResponse,
  AudioTrimRequest,
  BrowserEventPayload,
  LineHistoryPage,
  LineId,
  MinePrepareRequest,
  MinePrepareResponse,
  PublicConfig,
} from '@/api/types';

const TOKEN_STORAGE_KEY = 'textractor-media-bridge.session-token';

export class BridgeApiError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'BridgeApiError';
  }
}

export function loadSessionToken(): string {
  return (
    new URLSearchParams(window.location.search).get('token') ??
    localStorage.getItem(TOKEN_STORAGE_KEY) ??
    ''
  );
}

export function saveSessionToken(token: string | null | undefined): void {
  if (token) {
    localStorage.setItem(TOKEN_STORAGE_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_STORAGE_KEY);
  }
}

export function assetUrl(url: string, token: string): string {
  return withToken(url, token);
}

export async function getConfig(token: string): Promise<PublicConfig> {
  const config = await apiJson<PublicConfig>('/api/config', token);
  saveSessionToken(config.sessionToken);
  return config;
}

export async function updateAudioConfig(
  token: string,
  audioConfig: AudioConfig,
): Promise<PublicConfig> {
  const config = await apiJson<PublicConfig>('/api/config/audio', token, {
    method: 'POST',
    body: JSON.stringify(audioConfig),
  });
  saveSessionToken(config.sessionToken);
  return config;
}

export async function getLines(
  token: string,
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
  return apiJson<LineHistoryPage>(`/api/lines${suffix}`, token);
}

export async function finishAudio(token: string, lineId: LineId): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/finish`,
    token,
    { method: 'POST' },
  );
  return response.audio ?? null;
}

export async function getAudioTrimInfo(
  token: string,
  lineId: LineId,
): Promise<AudioTrimInfoResponse> {
  return apiJson<AudioTrimInfoResponse>(`/api/lines/${lineId}/audio/trim`, token);
}

export async function applyAudioTrim(
  token: string,
  lineId: LineId,
  request: AudioTrimRequest,
): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/trim`,
    token,
    {
      method: 'POST',
      body: JSON.stringify(request),
    },
  );
  return response.audio ?? null;
}

export async function finishTrimAudio(token: string, lineId: LineId): Promise<AudioState | null> {
  const response = await apiJson<{ lineId: LineId; audio?: AudioState | null }>(
    `/api/lines/${lineId}/audio/trim/finish`,
    token,
    { method: 'POST' },
  );
  return response.audio ?? null;
}

export async function prepareMine(
  token: string,
  request: MinePrepareRequest,
): Promise<MinePrepareResponse> {
  return apiJson<MinePrepareResponse>('/api/mine/prepare', token, {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

export async function getAssetBase64(token: string, assetId: string): Promise<AssetBase64Response> {
  return apiJson<AssetBase64Response>(`/api/assets/${assetId}/base64`, token, {
    method: 'POST',
  });
}

export function openEventSource(token: string): EventSource {
  return new EventSource(withToken('/api/events', token));
}

export function parseBrowserEvent(event: MessageEvent<string>): BrowserEventPayload {
  return JSON.parse(event.data) as BrowserEventPayload;
}

async function apiJson<T>(path: string, token: string, init: RequestInit = {}): Promise<T> {
  let response: Response;
  try {
    response = await fetch(withToken(path, token), {
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

function withToken(path: string, token: string): string {
  if (!token) {
    return path;
  }

  const separator = path.includes('?') ? '&' : '?';
  return `${path}${separator}token=${encodeURIComponent(token)}`;
}
