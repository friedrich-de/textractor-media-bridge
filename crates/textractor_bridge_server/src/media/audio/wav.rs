#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlicedWav {
    pub bytes: Vec<u8>,
    pub duration_ms: u64,
}

pub fn encode_pcm16_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
    let riff_len = 36u32.saturating_add(data_len);
    let byte_rate = sample_rate.saturating_mul(2);

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

pub fn slice_pcm16_wav(bytes: &[u8], start_ms: u64, end_ms: u64) -> Result<SlicedWav, String> {
    let wav = decode_pcm16_wav(bytes)?;
    let source_duration_ms = duration_ms(wav.samples.len(), wav.sample_rate);
    if start_ms >= end_ms || end_ms > source_duration_ms {
        return Err(format!(
            "invalid trim range {start_ms}..{end_ms}ms for {source_duration_ms}ms audio"
        ));
    }

    let start_index = ms_to_sample_index(start_ms, source_duration_ms, wav.samples.len());
    let end_index = ms_to_sample_index(end_ms, source_duration_ms, wav.samples.len());
    if end_index <= start_index {
        return Err("trim range produced no audio samples".to_owned());
    }

    let samples = wav.samples[start_index..end_index].to_vec();
    let duration_ms = duration_ms(samples.len(), wav.sample_rate);
    Ok(SlicedWav {
        bytes: encode_pcm16_wav(&samples, wav.sample_rate),
        duration_ms,
    })
}

pub(super) fn duration_ms(sample_count: usize, sample_rate: u32) -> u64 {
    (sample_count as u64).saturating_mul(1_000) / sample_rate as u64
}

pub(super) fn ms_to_sample_index(offset_ms: u64, duration_ms: u64, sample_count: usize) -> usize {
    (sample_count as u64)
        .saturating_mul(offset_ms)
        .saturating_div(duration_ms.max(1))
        .min(sample_count as u64) as usize
}

struct DecodedPcm16Wav {
    sample_rate: u32,
    samples: Vec<i16>,
}

fn decode_pcm16_wav(bytes: &[u8]) -> Result<DecodedPcm16Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("unsupported WAV file; expected RIFF/WAVE".to_owned());
    }

    let mut offset = 12usize;
    let mut sample_rate = None;
    let mut data = None;

    while offset.saturating_add(8) <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(chunk_len)
            .ok_or_else(|| "WAV chunk length overflow".to_owned())?;
        if data_end > bytes.len() {
            return Err("truncated WAV chunk".to_owned());
        }

        match chunk_id {
            b"fmt " => {
                if chunk_len < 16 {
                    return Err("invalid WAV fmt chunk".to_owned());
                }
                let audio_format = u16::from_le_bytes([bytes[data_start], bytes[data_start + 1]]);
                let channels = u16::from_le_bytes([bytes[data_start + 2], bytes[data_start + 3]]);
                let rate = u32::from_le_bytes([
                    bytes[data_start + 4],
                    bytes[data_start + 5],
                    bytes[data_start + 6],
                    bytes[data_start + 7],
                ]);
                let bits_per_sample =
                    u16::from_le_bytes([bytes[data_start + 14], bytes[data_start + 15]]);
                if audio_format != 1 || channels != 1 || bits_per_sample != 16 {
                    return Err("unsupported WAV format; expected PCM16 mono".to_owned());
                }
                sample_rate = Some(rate);
            }
            b"data" => {
                if chunk_len % 2 != 0 {
                    return Err("invalid PCM16 WAV data length".to_owned());
                }
                let samples = bytes[data_start..data_end]
                    .chunks_exact(2)
                    .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                    .collect::<Vec<_>>();
                data = Some(samples);
            }
            _ => {}
        }

        offset = data_end + usize::from(chunk_len % 2 == 1);
    }

    let sample_rate = sample_rate.ok_or_else(|| "WAV fmt chunk was not found".to_owned())?;
    let samples = data.ok_or_else(|| "WAV data chunk was not found".to_owned())?;
    Ok(DecodedPcm16Wav {
        sample_rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_encoder_writes_pcm16_header() {
        let wav = encode_pcm16_wav(&[0, i16::MAX, i16::MIN], 48_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn wav_slicer_crops_pcm16_ranges() {
        let samples = (0..48_000).map(|value| value as i16).collect::<Vec<_>>();
        let wav = encode_pcm16_wav(&samples, 48_000);

        let sliced = slice_pcm16_wav(&wav, 250, 750).unwrap();
        assert_eq!(sliced.duration_ms, 500);

        let decoded = decode_pcm16_wav(&sliced.bytes).unwrap();
        assert_eq!(decoded.samples.len(), 24_000);
        assert_eq!(decoded.samples[0], samples[12_000]);
    }

    #[test]
    fn wav_slicer_rejects_unsupported_audio() {
        assert!(slice_pcm16_wav(b"not a wav", 0, 100).is_err());
    }
}
