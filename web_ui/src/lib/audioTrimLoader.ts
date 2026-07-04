import { assetUrl, getAudioTrimInfo } from '@/api/bridge';
import type { AudioTrimInfoResponse, LineId } from '@/api/types';
import { buildActivityBars, decodePcm16Wav, type DecodedPcm16Wav } from '@/lib/wavPreview';

export interface LoadedAudioTrim {
  info: AudioTrimInfoResponse;
  decodedSource: DecodedPcm16Wav;
  activityBars: number[];
}

export async function loadAudioTrim(
  lineId: LineId,
  activityBarCount: number,
): Promise<LoadedAudioTrim> {
  const info = await getAudioTrimInfo(lineId);
  const response = await fetch(assetUrl(info.source.url));
  if (!response.ok) {
    throw new Error(`Unable to load audio graph: HTTP ${response.status}`);
  }

  const decodedSource = decodePcm16Wav(await response.arrayBuffer());
  return {
    info,
    decodedSource,
    activityBars: buildActivityBars(decodedSource, activityBarCount),
  };
}
