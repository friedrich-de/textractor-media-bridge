const API_VERSION = 6;

type JsonPrimitive = string | number | boolean | null;
type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

interface AnkiRequest {
  action: string;
  version: number;
  params?: JsonValue;
}

interface AnkiResponse<T = JsonValue> {
  result: T;
  error: string | null;
}

interface AnkiActionRequest {
  action: string;
  version: number;
  params: Record<string, JsonValue>;
}

interface AnkiActionResponse<T = JsonValue> {
  result: T;
  error: string | null;
}

export interface NoteInfo {
  noteId: number;
  modelName: string;
  tags: string[];
  fields: Record<string, { value: string; order: number }>;
}

export class AnkiConnectError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'AnkiConnectError';
  }
}

export async function requestPermission(endpoint: string): Promise<unknown> {
  return invoke(endpoint, 'requestPermission');
}

export async function getVersion(endpoint: string): Promise<number> {
  return invoke<number>(endpoint, 'version');
}

export async function getModelsWithFields(endpoint: string): Promise<Record<string, string[]>> {
  const modelNames = await invoke<string[]>(endpoint, 'modelNames');
  const actions = modelNames.map((modelName) => ({
    action: 'modelFieldNames',
    version: API_VERSION,
    params: { modelName },
  }));

  const results = await multiInvoke<string[]>(endpoint, actions);
  const modelsWithFields: Record<string, string[]> = {};
  modelNames.forEach((modelName, index) => {
    const item = results[index];
    modelsWithFields[modelName] = item && !item.error ? (item.result ?? []) : [];
  });

  return modelsWithFields;
}

export async function getLatestNote(
  endpoint: string,
  options: { deckName: string; modelName: string; searchDays?: number },
): Promise<NoteInfo | null> {
  const query = buildNoteQuery(options.deckName, options.modelName, options.searchDays ?? 7);
  const noteIds = await invoke<number[]>(endpoint, 'findNotes', { query });
  if (noteIds.length === 0) {
    return null;
  }

  const latest = [...noteIds].sort((a, b) => b - a)[0];
  const notes = await invoke<NoteInfo[]>(endpoint, 'notesInfo', { notes: [latest] });
  return notes[0] ?? null;
}

export async function storeMediaFile(
  endpoint: string,
  filename: string,
  data: string,
): Promise<string> {
  return invoke<string>(endpoint, 'storeMediaFile', { filename, data });
}

export async function updateNoteFields(
  endpoint: string,
  noteId: number,
  fields: Record<string, string>,
): Promise<null> {
  return invoke<null>(endpoint, 'updateNoteFields', {
    note: { id: noteId, fields },
  });
}

export async function guiBrowse(endpoint: string, query: string): Promise<number[]> {
  return invoke<number[]>(endpoint, 'guiBrowse', { query });
}

function buildNoteQuery(deckName: string, modelName: string, searchDays: number): string {
  const parts = [`added:${Math.max(1, Math.floor(searchDays))}`];
  if (deckName.trim()) {
    parts.unshift(`deck:"${deckName.trim()}"`);
  }
  if (modelName.trim()) {
    parts.unshift(`note:"${modelName.trim()}"`);
  }
  return parts.join(' ');
}

async function invoke<T>(endpoint: string, action: string, params?: JsonValue): Promise<T> {
  const request: AnkiRequest = { action, version: API_VERSION };
  if (params !== undefined) {
    request.params = params;
  }

  let response: Response;
  try {
    response = await fetch(endpoint, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new AnkiConnectError(`[${action}] Network error: ${message}`);
  }

  if (!response.ok) {
    throw new AnkiConnectError(`[${action}] HTTP ${response.status}: ${response.statusText}`);
  }

  let data: AnkiResponse<T>;
  try {
    data = (await response.json()) as AnkiResponse<T>;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new AnkiConnectError(`[${action}] Failed to parse response: ${message}`);
  }

  if (data.error) {
    throw new AnkiConnectError(`[${action}] ${data.error}`);
  }

  return data.result;
}

async function multiInvoke<T>(
  endpoint: string,
  actions: AnkiActionRequest[],
): Promise<Array<AnkiActionResponse<T>>> {
  return invoke<Array<AnkiActionResponse<T>>>(endpoint, 'multi', {
    actions: actions as unknown as JsonValue,
  });
}
