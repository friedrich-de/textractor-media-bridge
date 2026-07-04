import { computed, ref, watch, type ComputedRef, type Ref } from 'vue';

import { getAssetBase64, prepareMine } from '@/api/bridge';
import {
  getLatestNote,
  guiBrowse,
  storeMediaFile,
  updateNoteFields,
  type NoteInfo,
} from '@/api/ankiConnect';
import type { LineRecord, MinePrepareResponse } from '@/api/types';
import type { ToastApi } from '@/composables/useToast';
import { preserveHtmlTags, stripHtml } from '@/lib/htmlTags';
import type { MinerSettings } from '@/lib/minerSettings';

interface UseAnkiMiningOptions {
  settings: Ref<MinerSettings>;
  selectedLines: ComputedRef<readonly LineRecord[]>;
  selectedLineCount: ComputedRef<number>;
  clearSelection: () => void;
  toast: ToastApi;
}

export function useAnkiMining(options: UseAnkiMiningOptions) {
  const targetCardPreview = ref<string | null>(null);
  const loadingTargetCard = ref(false);
  const sendingToAnki = ref(false);
  const previewingMedia = ref(false);
  let previewRequestId = 0;

  const ankiConfigured = computed(() => {
    const anki = options.settings.value.anki;
    return Boolean(anki.modelName && (anki.sentenceField || anki.audioField || anki.imageField));
  });
  const canSendToAnki = computed(
    () => options.selectedLineCount.value > 0 && ankiConfigured.value && !sendingToAnki.value,
  );

  watch(
    [
      options.selectedLineCount,
      () => options.settings.value.anki.endpoint,
      () => options.settings.value.anki.deckName,
      () => options.settings.value.anki.modelName,
      () => options.settings.value.anki.frontField,
      () => options.settings.value.anki.maxLatestCardAgeMinutes,
    ],
    () => {
      void updateTargetCardPreview();
    },
    { immediate: true },
  );

  async function prepareSelection(): Promise<MinePrepareResponse> {
    const lineIds = options.selectedLines.value.map((line) => line.lineId);
    if (lineIds.length === 0) {
      throw new Error('No text rows selected.');
    }

    return prepareMine({
      lineIds,
      rangeSentenceSeparator: options.settings.value.anki.rangeSentenceSeparator,
      rangeScreenshotPick: options.settings.value.anki.rangeScreenshotPick,
    });
  }

  async function sendSelectionToAnki(): Promise<void> {
    if (!canSendToAnki.value) {
      options.toast.warning('Select text rows and configure Anki fields first.');
      return;
    }

    sendingToAnki.value = true;
    try {
      const prepared = await prepareSelection();
      const noteId = await updateLatestNote(prepared);

      options.toast.success(`Added ${prepared.lineIds.length} line(s) to Anki.`, {
        action: {
          label: 'Browse',
          onClick: () => {
            void guiBrowse(options.settings.value.anki.endpoint, `nid:${noteId}`);
          },
        },
      });
      options.clearSelection();
    } catch (error) {
      options.toast.error(error instanceof Error ? error.message : 'Unable to update Anki.');
    } finally {
      sendingToAnki.value = false;
    }
  }

  async function previewSelectionImage(): Promise<string | null> {
    previewingMedia.value = true;
    try {
      const prepared = await prepareSelection();
      return prepared.screenshot?.url ?? null;
    } finally {
      previewingMedia.value = false;
    }
  }

  async function previewSelectionAudio(): Promise<string | null> {
    previewingMedia.value = true;
    try {
      const prepared = await prepareSelection();
      return prepared.audio?.url ?? null;
    } finally {
      previewingMedia.value = false;
    }
  }

  async function updateTargetCardPreview(): Promise<void> {
    const requestId = ++previewRequestId;
    if (options.selectedLineCount.value === 0 || !ankiConfigured.value) {
      targetCardPreview.value = null;
      loadingTargetCard.value = false;
      return;
    }

    loadingTargetCard.value = true;
    try {
      const note = await getLatestNote(options.settings.value.anki.endpoint, {
        deckName: options.settings.value.anki.deckName,
        modelName: options.settings.value.anki.modelName,
      });
      if (requestId !== previewRequestId) {
        return;
      }

      if (!note || latestNoteTooOld(note, options.settings.value.anki.maxLatestCardAgeMinutes)) {
        targetCardPreview.value = null;
      } else {
        targetCardPreview.value = buildTargetCardPreview(
          note,
          options.settings.value.anki.frontField,
        );
      }
    } catch {
      if (requestId === previewRequestId) {
        targetCardPreview.value = null;
      }
    } finally {
      if (requestId === previewRequestId) {
        loadingTargetCard.value = false;
      }
    }
  }

  function resetTargetPreview(): void {
    previewRequestId += 1;
    targetCardPreview.value = null;
    loadingTargetCard.value = false;
  }

  async function updateLatestNote(prepared: MinePrepareResponse): Promise<number> {
    const anki = options.settings.value.anki;
    const note = await getLatestNote(anki.endpoint, {
      deckName: anki.deckName,
      modelName: anki.modelName,
    });
    if (!note || latestNoteTooOld(note, anki.maxLatestCardAgeMinutes)) {
      throw new Error('No recent Anki note found.');
    }

    const fields = await buildFields(prepared, note);
    if (Object.keys(fields).length === 0) {
      throw new Error('No Anki fields are configured for updates.');
    }

    await updateNoteFields(anki.endpoint, note.noteId, fields);
    return note.noteId;
  }

  async function buildFields(
    prepared: MinePrepareResponse,
    note: NoteInfo,
  ): Promise<Record<string, string>> {
    const anki = options.settings.value.anki;
    const fields: Record<string, string> = {};

    if (anki.sentenceField) {
      fields[anki.sentenceField] = preserveHtmlTags(
        noteFieldValue(note, anki.sentenceField),
        prepared.sentence,
      );
    }

    if (anki.sourceField) {
      fields[anki.sourceField] = prepared.source;
    }

    if (anki.imageField && prepared.screenshot) {
      const storedFilename = await storePreparedAsset(prepared.screenshot.assetId);
      fields[anki.imageField] = `<img src="${storedFilename}">`;
    }

    if (anki.audioField && prepared.audio) {
      const storedFilename = await storePreparedAsset(prepared.audio.assetId);
      fields[anki.audioField] = `[sound:${storedFilename}]`;
    }

    return fields;
  }

  async function storePreparedAsset(assetId: string): Promise<string> {
    const asset = await getAssetBase64(assetId);
    return storeMediaFile(options.settings.value.anki.endpoint, asset.filename, asset.data);
  }

  return {
    targetCardPreview,
    loadingTargetCard,
    sendingToAnki,
    previewingMedia,
    ankiConfigured,
    canSendToAnki,
    prepareSelection,
    previewSelectionImage,
    previewSelectionAudio,
    sendSelectionToAnki,
    updateTargetCardPreview,
    resetTargetPreview,
  };
}

function latestNoteTooOld(note: NoteInfo, maxAgeMinutes: number): boolean {
  return maxAgeMinutes > 0 && Date.now() - note.noteId > maxAgeMinutes * 60_000;
}

function noteFieldValue(note: NoteInfo, fieldName: string): string {
  return note.fields[fieldName]?.value ?? '';
}

function buildTargetCardPreview(note: NoteInfo, frontField: string): string {
  const rawPreview =
    (frontField ? note.fields[frontField]?.value : undefined) ??
    Object.values(note.fields).find((field) => field.value)?.value ??
    `Note ${note.noteId}`;
  const stripped = stripHtml(rawPreview);
  return stripped.length > 54 ? `${stripped.slice(0, 54)}...` : stripped;
}
