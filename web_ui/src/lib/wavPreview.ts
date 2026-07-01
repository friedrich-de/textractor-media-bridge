export interface DecodedPcm16Wav {
  view: DataView;
  channels: number;
  sampleRate: number;
  bytesPerFrame: number;
  dataOffset: number;
  sampleCount: number;
}

export function decodePcm16Wav(bytes: ArrayBuffer): DecodedPcm16Wav {
  const view = new DataView(bytes);
  if (
    bytes.byteLength < 44 ||
    readAscii(view, 0, 4) !== 'RIFF' ||
    readAscii(view, 8, 4) !== 'WAVE'
  ) {
    throw new Error('Unable to read audio graph: unsupported WAV file.');
  }

  let channels = 0;
  let sampleRate = 0;
  let bitsPerSample = 0;
  let dataOffset = 0;
  let dataLength = 0;
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const chunkId = readAscii(view, offset, 4);
    const chunkLength = view.getUint32(offset + 4, true);
    const chunkData = offset + 8;
    if (chunkData + chunkLength > bytes.byteLength) {
      throw new Error('Unable to read audio graph: truncated WAV file.');
    }

    if (chunkId === 'fmt ') {
      const audioFormat = view.getUint16(chunkData, true);
      channels = view.getUint16(chunkData + 2, true);
      sampleRate = view.getUint32(chunkData + 4, true);
      bitsPerSample = view.getUint16(chunkData + 14, true);
      if (audioFormat !== 1 || bitsPerSample !== 16 || channels < 1) {
        throw new Error('Unable to read audio graph: expected PCM16 WAV audio.');
      }
    } else if (chunkId === 'data') {
      dataOffset = chunkData;
      dataLength = chunkLength;
    }

    offset = chunkData + chunkLength + (chunkLength % 2);
  }

  if (!channels || !dataOffset || !dataLength) {
    throw new Error('Unable to read audio graph: missing WAV audio data.');
  }

  const bytesPerFrame = channels * 2;
  return {
    view,
    channels,
    sampleRate,
    bytesPerFrame,
    dataOffset,
    sampleCount: Math.floor(dataLength / bytesPerFrame),
  };
}

export function buildActivityBars(wav: DecodedPcm16Wav, barCount: number): number[] {
  const bars = Array.from({ length: barCount }, (_, index) => {
    const start = Math.floor((index / barCount) * wav.sampleCount);
    const end = Math.max(start + 1, Math.floor(((index + 1) / barCount) * wav.sampleCount));
    let sumSquares = 0;
    let count = 0;
    for (let sampleIndex = start; sampleIndex < end; sampleIndex += 1) {
      const offset = wav.dataOffset + sampleIndex * wav.bytesPerFrame;
      let peak = 0;
      for (let channel = 0; channel < wav.channels; channel += 1) {
        const value = wav.view.getInt16(offset + channel * 2, true) / 32768;
        peak = Math.max(peak, Math.abs(value));
      }
      sumSquares += peak * peak;
      count += 1;
    }
    return Math.sqrt(sumSquares / Math.max(1, count));
  });

  const sorted = [...bars].sort((a, b) => a - b);
  const scale = sorted[Math.floor(sorted.length * 0.95)] || sorted.at(-1) || 1;
  return bars.map((bar) => Math.min(1, Math.sqrt(bar / Math.max(scale, 0.001))));
}

export function createPcm16WavClipUrl(
  wav: DecodedPcm16Wav,
  startMs: number,
  endMs: number,
): string {
  const startFrame = msToFrame(startMs, wav);
  const endFrame = Math.max(startFrame + 1, msToFrame(endMs, wav));
  const dataStart = wav.dataOffset + startFrame * wav.bytesPerFrame;
  const dataEnd = wav.dataOffset + Math.min(endFrame, wav.sampleCount) * wav.bytesPerFrame;
  const dataBytes = new Uint8Array(dataEnd - dataStart);
  dataBytes.set(new Uint8Array(wav.view.buffer, dataStart, dataBytes.byteLength));
  const header = wavHeader(dataBytes.byteLength, wav.channels, wav.sampleRate);
  return URL.createObjectURL(new Blob([header, dataBytes], { type: 'audio/wav' }));
}

function msToFrame(ms: number, wav: DecodedPcm16Wav): number {
  return clampNumber((ms * wav.sampleRate) / 1_000, 0, wav.sampleCount);
}

function wavHeader(dataLength: number, channels: number, sampleRate: number): ArrayBuffer {
  const header = new ArrayBuffer(44);
  const view = new DataView(header);
  writeAscii(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataLength, true);
  writeAscii(view, 8, 'WAVE');
  writeAscii(view, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * 2, true);
  view.setUint16(32, channels * 2, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, 'data');
  view.setUint32(40, dataLength, true);
  return header;
}

function readAscii(view: DataView, offset: number, length: number): string {
  return Array.from({ length }, (_, index) =>
    String.fromCharCode(view.getUint8(offset + index)),
  ).join('');
}

function writeAscii(view: DataView, offset: number, value: string): void {
  for (let index = 0; index < value.length; index += 1) {
    view.setUint8(offset + index, value.charCodeAt(index));
  }
}

function clampNumber(value: number, min: number, max: number): number {
  const parsed = Number.isFinite(value) ? value : min;
  return Math.min(Math.max(Math.round(parsed), min), max);
}
