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
import { buildFilteredSentence } from '@/lib/textFilters';

interface UseAnkiMiningOptions {
  settings: Ref<MinerSettings>;
  selectedLines: ComputedRef<readonly LineRecord[]>;
  selectedLineCount: ComputedRef<number>;
  clearSelection: () => void;
  toast: ToastApi;
}

export function useAnkiMining(options: UseAnkiMiningOptions) {
  const targetCardPreview = ref<string | null>(null);
  const targetCardError = ref<string | null>(null);
  const loadingTargetCard = ref(false);
  const sendingToAnki = ref(false);
  const previewingMedia = ref(false);
  let previewRequestId = 0;

  const ankiConfigured = computed(() => {
    const anki = options.settings.value.anki;
    return Boolean(anki.modelName && configuredUpdateFields(anki).length > 0);
  });
  const canSendToAnki = computed(
    () => options.selectedLineCount.value > 0 && ankiConfigured.value && !sendingToAnki.value,
  );

  watch(
    [
      options.selectedLineCount,
      () => options.settings.value.anki.endpoint,
      () => options.settings.value.anki.modelName,
      () => options.settings.value.anki.frontField,
      () => options.settings.value.anki.sentenceField,
      () => options.settings.value.anki.audioField,
      () => options.settings.value.anki.imageField,
      () => options.settings.value.anki.sourceField,
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

    const prepared = await prepareMine({
      lineIds,
      rangeSentenceSeparator: options.settings.value.anki.rangeSentenceSeparator,
      rangeScreenshotPick: options.settings.value.anki.rangeScreenshotPick,
    });
    return {
      ...prepared,
      sentence: buildFilteredSentence(
        options.selectedLines.value,
        options.settings.value.anki.rangeSentenceSeparator,
      ),
    };
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
      targetCardError.value = null;
      loadingTargetCard.value = false;
      return;
    }

    loadingTargetCard.value = true;
    targetCardError.value = null;
    try {
      const note = await getLatestNote(options.settings.value.anki.endpoint, {
        modelName: options.settings.value.anki.modelName,
        configuredFields: configuredUpdateFields(options.settings.value.anki),
      });
      if (requestId !== previewRequestId) {
        return;
      }

      if (!note) {
        targetCardPreview.value = null;
        targetCardError.value = 'No recent compatible note';
      } else if (latestNoteTooOld(note, options.settings.value.anki.maxLatestCardAgeMinutes)) {
        targetCardPreview.value = null;
        targetCardError.value = `Target older than ${options.settings.value.anki.maxLatestCardAgeMinutes} min`;
      } else {
        targetCardPreview.value = buildTargetCardPreview(
          note,
          options.settings.value.anki.frontField,
        );
        targetCardError.value = null;
      }
    } catch (error) {
      if (requestId === previewRequestId) {
        targetCardPreview.value = null;
        targetCardError.value =
          error instanceof Error ? error.message : 'Unable to check Anki target';
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
    targetCardError.value = null;
    loadingTargetCard.value = false;
  }

  async function updateLatestNote(prepared: MinePrepareResponse): Promise<number> {
    const anki = options.settings.value.anki;
    const note = await getLatestNote(anki.endpoint, {
      modelName: anki.modelName,
      configuredFields: configuredUpdateFields(anki),
    });
    if (!note) {
      throw new Error('No recent compatible Anki note found.');
    }
    if (latestNoteTooOld(note, anki.maxLatestCardAgeMinutes)) {
      throw new Error(
        `Newest compatible Anki note is older than ${anki.maxLatestCardAgeMinutes} minutes.`,
      );
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
    const sentenceField = anki.sentenceField.trim();
    const sourceField = anki.sourceField.trim();
    const imageField = anki.imageField.trim();
    const audioField = anki.audioField.trim();

    if (sentenceField && noteHasField(note, sentenceField)) {
      fields[sentenceField] = preserveHtmlTags(
        noteFieldValue(note, sentenceField),
        prepared.sentence,
      );
    }

    if (sourceField && noteHasField(note, sourceField)) {
      fields[sourceField] = prepared.source;
    }

    if (imageField && prepared.screenshot && noteHasField(note, imageField)) {
      const storedFilename = await storePreparedAsset(prepared.screenshot.assetId);
      fields[imageField] = `<img src="${storedFilename}">`;
    }

    if (audioField && prepared.audio && noteHasField(note, audioField)) {
      const storedFilename = await storePreparedAsset(prepared.audio.assetId);
      fields[audioField] = `[sound:${storedFilename}]`;
    }

    return fields;
  }

  async function storePreparedAsset(assetId: string): Promise<string> {
    const asset = await getAssetBase64(assetId);
    return storeMediaFile(options.settings.value.anki.endpoint, asset.filename, asset.data);
  }

  return {
    targetCardPreview,
    targetCardError,
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

function configuredUpdateFields(anki: MinerSettings['anki']): string[] {
  return [anki.sentenceField, anki.audioField, anki.imageField, anki.sourceField]
    .map((field) => field.trim())
    .filter((field) => field.length > 0);
}

function noteFieldValue(note: NoteInfo, fieldName: string): string {
  return note.fields[fieldName]?.value ?? '';
}

function noteHasField(note: NoteInfo, fieldName: string): boolean {
  return Boolean(fieldName && note.fields[fieldName]);
}

function buildTargetCardPreview(note: NoteInfo, frontField: string): string {
  const rawPreview =
    (frontField.trim() ? note.fields[frontField.trim()]?.value : undefined) ??
    Object.values(note.fields).find((field) => field.value)?.value ??
    `Note ${note.noteId}`;
  const stripped = stripHtml(rawPreview);
  return stripped.length > 54 ? `${stripped.slice(0, 54)}...` : stripped;
}
