#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrimmedSamples {
    pub samples: Vec<i16>,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ActivityStats {
    first_active_index: usize,
    pub last_active_index: usize,
    active_samples: usize,
}

pub(super) fn trim_to_activity(
    samples: &[i16],
    sample_rate: u32,
    activity_threshold: u16,
    min_activity_ms: u64,
    padding_ms: u64,
) -> Option<TrimmedSamples> {
    let stats = activity_stats(samples, activity_threshold)?;
    if !has_min_activity(&stats, sample_rate, min_activity_ms) {
        return None;
    }

    let padding = (sample_rate as u64 * padding_ms / 1_000) as usize;
    let start = stats.first_active_index.saturating_sub(padding);
    let end = stats
        .last_active_index
        .saturating_add(padding)
        .saturating_add(1)
        .min(samples.len());
    Some(TrimmedSamples {
        samples: samples[start..end].to_vec(),
        start_ms: duration_ms(start, sample_rate),
        end_ms: duration_ms(end, sample_rate),
    })
}

pub(super) fn activity_stats(samples: &[i16], activity_threshold: u16) -> Option<ActivityStats> {
    let mut first_active_index = None;
    let mut last_active_index = 0usize;
    let mut active_samples = 0usize;
    for (index, sample) in samples.iter().enumerate() {
        if sample.unsigned_abs() >= activity_threshold {
            first_active_index.get_or_insert(index);
            last_active_index = index;
            active_samples += 1;
        }
    }

    first_active_index.map(|first_active_index| ActivityStats {
        first_active_index,
        last_active_index,
        active_samples,
    })
}

pub(super) fn has_min_activity(
    stats: &ActivityStats,
    sample_rate: u32,
    min_activity_ms: u64,
) -> bool {
    stats.active_samples >= min_active_samples(sample_rate, min_activity_ms)
}

fn min_active_samples(sample_rate: u32, min_activity_ms: u64) -> usize {
    ((sample_rate as u64 * min_activity_ms / 1_000) as usize).max(1)
}

fn duration_ms(sample_count: usize, sample_rate: u32) -> u64 {
    (sample_count as u64).saturating_mul(1_000) / sample_rate as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    #[test]
    fn activity_requires_minimum_active_samples() {
        let mut samples = vec![0; 4_800];
        samples[2_000] = 1_000;

        let stats = activity_stats(&samples, 300).expect("active sample");

        assert!(!has_min_activity(&stats, SAMPLE_RATE, 30));
        assert!(has_min_activity(&stats, SAMPLE_RATE, 0));
    }

    #[test]
    fn trim_to_activity_keeps_padding_and_rejects_silence() {
        assert!(trim_to_activity(&vec![0; 4_800], SAMPLE_RATE, 300, 30, 1_000).is_none());

        let mut samples = vec![0; 100_000];
        for sample in samples.iter_mut().take(52_000).skip(50_000) {
            *sample = 1_000;
        }
        let trimmed = trim_to_activity(&samples, SAMPLE_RATE, 300, 30, 1_000).unwrap();
        assert_eq!(trimmed.samples.len(), 98_000);
        assert_eq!(trimmed.start_ms, 41);
        assert_eq!(trimmed.end_ms, 2083);
        assert!(trimmed.samples.len() < samples.len());
    }
}
