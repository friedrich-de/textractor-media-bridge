use bridge_protocol::{AudioEndReason, AudioState, LineId};
use parking_lot::{Mutex, RwLock};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    thread,
};

use crate::{config::AudioConfig, time::unix_ms_now};

const CAPTURE_SAMPLE_RATE: u32 = 48_000;
const CAPTURE_CHANNELS: u16 = 2;
const CAPTURE_CHUNK_FRAMES: usize = 480;

#[derive(Clone)]
pub struct AudioManager {
    sessions: Arc<Mutex<HashMap<LineId, AudioSession>>>,
    workers: Arc<Mutex<HashMap<u32, CaptureWorker>>>,
    config: Arc<RwLock<AudioConfig>>,
}

impl AudioManager {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            config: Arc::new(RwLock::new(config)),
        }
    }

    pub fn update_config(&self, config: AudioConfig) {
        *self.config.write() = config;
    }

    pub fn enabled(&self) -> bool {
        self.config.read().backend != "off"
    }

    pub fn start_line_session(
        &self,
        line_id: LineId,
        process_id: u32,
        started_unix_ms: i64,
    ) -> Option<AudioState> {
        if !self.enabled() {
            return None;
        }

        let config = self.config.read().clone();
        if BackendPreference::from_config(&config.backend).is_none() {
            return Some(AudioState::NoAudio {
                reason: Some(format!("unsupported audio backend '{}'", config.backend)),
            });
        }

        self.ensure_worker(process_id, config);
        self.sessions.lock().insert(
            line_id,
            AudioSession {
                process_id,
                started_unix_ms,
                main_finished: false,
            },
        );
        Some(AudioState::Recording { started_unix_ms })
    }

    pub fn finish_main_line_session(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
    ) -> Option<FinishedMainAudio> {
        self.finish_main_line_session_at(line_id, reason, unix_ms_now())
    }

    pub fn finish_main_line_session_at(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
        end_unix_ms: i64,
    ) -> Option<FinishedMainAudio> {
        let session = {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&line_id)?;
            if session.main_finished {
                return None;
            }
            session.main_finished = true;
            session.clone()
        };
        let worker = self.workers.lock().get(&session.process_id).cloned();
        let Some(worker) = worker else {
            self.sessions.lock().remove(&line_id);
            return Some(FinishedMainAudio::NoAudio {
                reason: format!(
                    "audio capture worker for process {} was unavailable",
                    session.process_id
                ),
            });
        };

        let config = self.config.read().clone();
        let main_start_unix_ms = session
            .started_unix_ms
            .saturating_sub(ms_to_i64(config.ready_preroll_ms));
        let main_samples = worker
            .shared
            .buffer
            .lock()
            .collect_range(main_start_unix_ms, end_unix_ms);

        let status = worker.shared.status.lock().clone();
        let Some(trimmed) = trim_to_activity(
            &main_samples,
            CAPTURE_SAMPLE_RATE,
            config.activity_threshold,
            config.min_activity_ms,
            config.trim_padding_ms,
        ) else {
            let reason = no_audio_reason(session.process_id, main_samples.len(), status);
            self.sessions.lock().remove(&line_id);
            return Some(FinishedMainAudio::NoAudio { reason });
        };

        let duration_ms = duration_ms(trimmed.samples.len(), CAPTURE_SAMPLE_RATE);
        Some(FinishedMainAudio::Ready(CapturedMainAudio {
            bytes: encode_pcm16_wav(&trimmed.samples, CAPTURE_SAMPLE_RATE),
            duration_ms,
            end_reason: reason,
            mime_type: "audio/wav",
            trim_recording_started_unix_ms: session
                .started_unix_ms
                .saturating_sub(ms_to_i64(config.trim_source_preroll_ms)),
        }))
    }

    pub fn finish_trim_line_session(
        &self,
        line_id: LineId,
        reason: AudioEndReason,
    ) -> Option<FinishedTrimAudio> {
        self.finish_trim_line_session_at(line_id, reason, unix_ms_now())
    }

    pub fn finish_trim_line_session_at(
        &self,
        line_id: LineId,
        _reason: AudioEndReason,
        end_unix_ms: i64,
    ) -> Option<FinishedTrimAudio> {
        let session = self.sessions.lock().remove(&line_id)?;
        let worker = self.workers.lock().get(&session.process_id).cloned();
        let Some(worker) = worker else {
            return Some(FinishedTrimAudio::NoAudio {
                reason: format!(
                    "audio capture worker for process {} was unavailable",
                    session.process_id
                ),
            });
        };

        let config = self.config.read().clone();
        let source_start_unix_ms = session
            .started_unix_ms
            .saturating_sub(ms_to_i64(config.trim_source_preroll_ms));
        let source_samples = worker
            .shared
            .buffer
            .lock()
            .collect_range(source_start_unix_ms, end_unix_ms);
        if source_samples.is_empty() {
            return Some(FinishedTrimAudio::NoAudio {
                reason: format!(
                    "no trim audio samples captured for process {}",
                    session.process_id
                ),
            });
        }

        let source_duration_ms = duration_ms(source_samples.len(), CAPTURE_SAMPLE_RATE);
        Some(FinishedTrimAudio::Ready(CapturedTrimAudio {
            source_bytes: encode_pcm16_wav(&source_samples, CAPTURE_SAMPLE_RATE),
            source_duration_ms,
            start_ms: 0,
            end_ms: source_duration_ms,
            mime_type: "audio/wav",
        }))
    }

    pub fn recording_line_ids_for_process(&self, process_id: u32) -> Vec<LineId> {
        self.sessions
            .lock()
            .iter()
            .filter_map(|(line_id, session)| (session.process_id == process_id).then_some(*line_id))
            .collect()
    }

    pub fn is_recording(&self, line_id: LineId) -> bool {
        self.sessions.lock().contains_key(&line_id)
    }

    pub fn line_end_reason(&self, line_id: LineId) -> Option<AudioEndReason> {
        let session = self.sessions.lock().get(&line_id).cloned()?;
        if session.main_finished {
            return None;
        }
        let now = unix_ms_now();
        let elapsed_ms = now.saturating_sub(session.started_unix_ms).max(0) as u64;
        let config = self.config.read().clone();
        if elapsed_ms >= config.max_duration_ms {
            return Some(AudioEndReason::MaxDuration);
        }

        let worker = self.workers.lock().get(&session.process_id).cloned()?;
        let samples = worker
            .shared
            .buffer
            .lock()
            .collect_range(session.started_unix_ms, now);

        match activity_stats(&samples, config.trim_activity_threshold) {
            Some(stats)
                if stats.active_samples
                    >= min_active_samples(CAPTURE_SAMPLE_RATE, config.trim_min_activity_ms) =>
            {
                let trailing_silence_ms = duration_ms(
                    samples.len().saturating_sub(stats.last_active_index + 1),
                    CAPTURE_SAMPLE_RATE,
                );
                (trailing_silence_ms >= config.trailing_silence_ms)
                    .then_some(AudioEndReason::Silence)
            }
            _ => (elapsed_ms >= config.no_speech_timeout_ms)
                .then_some(AudioEndReason::NoSpeechTimeout),
        }
    }

    pub fn trim_line_end_reason(&self, line_id: LineId) -> Option<AudioEndReason> {
        let session = self.sessions.lock().get(&line_id).cloned()?;
        let now = unix_ms_now();
        let elapsed_ms = now.saturating_sub(session.started_unix_ms).max(0) as u64;
        let config = self.config.read().clone();
        if elapsed_ms >= config.max_duration_ms {
            return Some(AudioEndReason::MaxDuration);
        }

        let worker = self.workers.lock().get(&session.process_id).cloned()?;
        let samples = worker
            .shared
            .buffer
            .lock()
            .collect_range(session.started_unix_ms, now);

        match activity_stats(&samples, config.activity_threshold) {
            Some(stats)
                if stats.active_samples
                    >= min_active_samples(CAPTURE_SAMPLE_RATE, config.min_activity_ms) =>
            {
                let trailing_silence_ms = duration_ms(
                    samples.len().saturating_sub(stats.last_active_index + 1),
                    CAPTURE_SAMPLE_RATE,
                );
                (trailing_silence_ms >= config.trim_trailing_silence_ms)
                    .then_some(AudioEndReason::Silence)
            }
            _ => (elapsed_ms >= config.trim_no_speech_timeout_ms)
                .then_some(AudioEndReason::NoSpeechTimeout),
        }
    }

    fn ensure_worker(&self, process_id: u32, config: AudioConfig) {
        let retention_ms = self.retention_ms(&config);
        let mut workers = self.workers.lock();
        workers
            .entry(process_id)
            .or_insert_with(|| CaptureWorker::spawn(process_id, config, retention_ms));
    }

    fn retention_ms(&self, config: &AudioConfig) -> i64 {
        config
            .max_duration_ms
            .saturating_add(config.no_speech_timeout_ms)
            .saturating_add(config.trim_no_speech_timeout_ms)
            .saturating_add(config.ready_preroll_ms)
            .saturating_add(config.trim_source_preroll_ms)
            .saturating_add(10_000)
            .max(30_000)
            .min(i64::MAX as u64) as i64
    }
}

#[derive(Debug, Clone)]
pub enum FinishedMainAudio {
    Ready(CapturedMainAudio),
    NoAudio { reason: String },
}

#[derive(Debug, Clone)]
pub enum FinishedTrimAudio {
    Ready(CapturedTrimAudio),
    NoAudio { reason: String },
}

#[derive(Debug, Clone)]
pub struct CapturedMainAudio {
    pub bytes: Vec<u8>,
    pub duration_ms: u64,
    pub end_reason: AudioEndReason,
    pub mime_type: &'static str,
    pub trim_recording_started_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct CapturedTrimAudio {
    pub source_bytes: Vec<u8>,
    pub source_duration_ms: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub mime_type: &'static str,
}

#[derive(Debug, Clone)]
struct AudioSession {
    process_id: u32,
    started_unix_ms: i64,
    main_finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendPreference {
    Auto,
    ProcessLoopback,
    SystemLoopback,
}

impl BackendPreference {
    fn from_config(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "process-loopback" | "process_loopback" | "wasapi-process" => {
                Some(Self::ProcessLoopback)
            }
            "system-loopback" | "system_loopback" | "wasapi-loopback" => Some(Self::SystemLoopback),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct CaptureWorker {
    shared: Arc<CaptureShared>,
}

impl CaptureWorker {
    fn spawn(process_id: u32, config: AudioConfig, retention_ms: i64) -> Self {
        let shared = Arc::new(CaptureShared::new(retention_ms));
        let worker = Self {
            shared: shared.clone(),
        };
        thread::Builder::new()
            .name(format!("audio-capture-{process_id}"))
            .spawn(move || run_capture_worker(process_id, config, shared))
            .expect("failed to spawn audio capture worker");
        worker
    }
}

struct CaptureShared {
    buffer: Mutex<RollingAudioBuffer>,
    status: Mutex<CaptureStatus>,
}

impl CaptureShared {
    fn new(retention_ms: i64) -> Self {
        Self {
            buffer: Mutex::new(RollingAudioBuffer::new(retention_ms)),
            status: Mutex::new(CaptureStatus::default()),
        }
    }

    fn set_ready(&self, backend: &'static str, warning: Option<String>) {
        let mut status = self.status.lock();
        status.ready = true;
        status.backend = Some(backend);
        status.last_error = warning;
    }

    fn set_error(&self, message: String) {
        let mut status = self.status.lock();
        status.ready = false;
        status.last_error = Some(message);
    }
}

#[derive(Debug, Clone, Default)]
struct CaptureStatus {
    ready: bool,
    backend: Option<&'static str>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct AudioChunk {
    start_unix_ms: i64,
    end_unix_ms: i64,
    samples: Vec<i16>,
}

#[derive(Debug, Clone)]
struct RollingAudioBuffer {
    chunks: VecDeque<AudioChunk>,
    retention_ms: i64,
}

impl RollingAudioBuffer {
    fn new(retention_ms: i64) -> Self {
        Self {
            chunks: VecDeque::new(),
            retention_ms,
        }
    }

    fn push_chunk(&mut self, chunk: AudioChunk) {
        let cutoff = chunk.end_unix_ms.saturating_sub(self.retention_ms);
        self.chunks.push_back(chunk);
        while self
            .chunks
            .front()
            .is_some_and(|front| front.end_unix_ms < cutoff)
        {
            self.chunks.pop_front();
        }
    }

    fn collect_range(&self, start_unix_ms: i64, end_unix_ms: i64) -> Vec<i16> {
        let mut out = Vec::new();
        for chunk in &self.chunks {
            if chunk.end_unix_ms <= start_unix_ms || chunk.start_unix_ms >= end_unix_ms {
                continue;
            }

            let chunk_duration_ms = chunk.end_unix_ms.saturating_sub(chunk.start_unix_ms);
            if chunk_duration_ms <= 0 || chunk.samples.is_empty() {
                continue;
            }

            let start_offset_ms = start_unix_ms
                .saturating_sub(chunk.start_unix_ms)
                .clamp(0, chunk_duration_ms);
            let end_offset_ms = end_unix_ms
                .saturating_sub(chunk.start_unix_ms)
                .clamp(0, chunk_duration_ms);
            if end_offset_ms <= start_offset_ms {
                continue;
            }

            let start_index = ms_to_sample_index(
                start_offset_ms as u64,
                chunk_duration_ms as u64,
                chunk.samples.len(),
            );
            let end_index = ms_to_sample_index(
                end_offset_ms as u64,
                chunk_duration_ms as u64,
                chunk.samples.len(),
            )
            .min(chunk.samples.len());
            if end_index > start_index {
                out.extend_from_slice(&chunk.samples[start_index..end_index]);
            }
        }
        out
    }
}

fn run_capture_worker(process_id: u32, config: AudioConfig, shared: Arc<CaptureShared>) {
    #[cfg(windows)]
    {
        if let Err(error) = windows_capture::run(process_id, config, shared.clone()) {
            shared.set_error(error);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = process_id;
        let _ = config;
        shared.set_error("WASAPI audio capture is only available on Windows".to_owned());
    }
}

fn no_audio_reason(process_id: u32, sample_count: usize, status: CaptureStatus) -> String {
    if let Some(error) = status.last_error {
        return error;
    }
    if !status.ready {
        return format!("audio capture for process {process_id} is still starting");
    }
    if sample_count == 0 {
        return format!("no audio samples captured for process {process_id}");
    }
    "captured audio did not contain enough non-silent samples".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrimmedSamples {
    samples: Vec<i16>,
    start_ms: u64,
    end_ms: u64,
}

fn trim_to_activity(
    samples: &[i16],
    sample_rate: u32,
    activity_threshold: u16,
    min_activity_ms: u64,
    padding_ms: u64,
) -> Option<TrimmedSamples> {
    let stats = activity_stats(samples, activity_threshold)?;
    if stats.active_samples < min_active_samples(sample_rate, min_activity_ms) {
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

#[derive(Debug, Clone, Copy)]
struct ActivityStats {
    first_active_index: usize,
    last_active_index: usize,
    active_samples: usize,
}

fn activity_stats(samples: &[i16], activity_threshold: u16) -> Option<ActivityStats> {
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

fn min_active_samples(sample_rate: u32, min_activity_ms: u64) -> usize {
    ((sample_rate as u64 * min_activity_ms / 1_000) as usize).max(1)
}

fn duration_ms(sample_count: usize, sample_rate: u32) -> u64 {
    (sample_count as u64).saturating_mul(1_000) / sample_rate as u64
}

fn ms_to_i64(ms: u64) -> i64 {
    ms.min(i64::MAX as u64) as i64
}

fn ms_to_sample_index(offset_ms: u64, duration_ms: u64, sample_count: usize) -> usize {
    (sample_count as u64)
        .saturating_mul(offset_ms)
        .saturating_div(duration_ms.max(1))
        .min(sample_count as u64) as usize
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlicedWav {
    pub bytes: Vec<u8>,
    pub duration_ms: u64,
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

#[cfg(windows)]
mod windows_capture {
    use super::{
        duration_ms, unix_ms_now, AudioChunk, BackendPreference, CaptureShared, CAPTURE_CHANNELS,
        CAPTURE_CHUNK_FRAMES, CAPTURE_SAMPLE_RATE,
    };
    use crate::config::AudioConfig;
    use std::{collections::VecDeque, sync::Arc};
    use wasapi::{
        initialize_mta, AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, SampleType,
        StreamMode, WaveFormat,
    };

    struct InitializedCapture {
        audio_client: AudioClient,
        backend: &'static str,
        warning: Option<String>,
        blockalign: usize,
    }

    pub fn run(
        process_id: u32,
        config: AudioConfig,
        shared: Arc<CaptureShared>,
    ) -> Result<(), String> {
        initialize_mta()
            .ok()
            .map_err(|error| format!("COM initialization failed for audio capture: {error:?}"))?;

        let mut initialized = initialize_capture(process_id, &config)?;
        let h_event = initialized
            .audio_client
            .set_get_eventhandle()
            .map_err(|error| format!("failed to create WASAPI event handle: {error}"))?;
        let capture_client = initialized
            .audio_client
            .get_audiocaptureclient()
            .map_err(|error| format!("failed to get WASAPI capture client: {error}"))?;

        initialized
            .audio_client
            .start_stream()
            .map_err(|error| format!("failed to start WASAPI stream: {error}"))?;
        shared.set_ready(initialized.backend, initialized.warning.take());

        let mut sample_queue = VecDeque::new();
        loop {
            drain_packets(
                &capture_client,
                initialized.blockalign,
                &mut sample_queue,
                &shared,
            )?;
            if h_event.wait_for_event(250).is_err() {
                continue;
            }
        }
    }

    fn initialize_capture(
        process_id: u32,
        config: &AudioConfig,
    ) -> Result<InitializedCapture, String> {
        let preference = BackendPreference::from_config(&config.backend)
            .ok_or_else(|| format!("unsupported audio backend '{}'", config.backend))?;

        match preference {
            BackendPreference::Auto => match initialize_process_loopback(process_id) {
                Ok(capture) => Ok(capture),
                Err(process_error) => {
                    let mut capture = initialize_system_loopback()?;
                    capture.warning = Some(format!(
                        "process loopback unavailable ({process_error}); using system loopback"
                    ));
                    Ok(capture)
                }
            },
            BackendPreference::ProcessLoopback => initialize_process_loopback(process_id),
            BackendPreference::SystemLoopback => initialize_system_loopback(),
        }
    }

    fn initialize_process_loopback(process_id: u32) -> Result<InitializedCapture, String> {
        let desired_format = desired_format();
        let mut audio_client = AudioClient::new_application_loopback_client(process_id, true)
            .map_err(|error| {
                format!("failed to activate process loopback for PID {process_id}: {error}")
            })?;
        initialize_client(&mut audio_client, &desired_format, 0)?;
        Ok(InitializedCapture {
            audio_client,
            backend: "process-loopback",
            warning: None,
            blockalign: desired_format.get_blockalign() as usize,
        })
    }

    fn initialize_system_loopback() -> Result<InitializedCapture, String> {
        let desired_format = desired_format();
        let enumerator = DeviceEnumerator::new()
            .map_err(|error| format!("failed to create WASAPI device enumerator: {error}"))?;
        let device = enumerator
            .get_default_device(&Direction::Render)
            .map_err(|error| format!("failed to get default render device: {error}"))?;
        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|error| format!("failed to open default render device: {error}"))?;
        let buffer_duration_hns = audio_client
            .get_device_period()
            .map(|(_, min)| min)
            .unwrap_or(0);
        initialize_client(&mut audio_client, &desired_format, buffer_duration_hns)?;
        Ok(InitializedCapture {
            audio_client,
            backend: "system-loopback",
            warning: None,
            blockalign: desired_format.get_blockalign() as usize,
        })
    }

    fn desired_format() -> WaveFormat {
        WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            CAPTURE_SAMPLE_RATE as usize,
            CAPTURE_CHANNELS as usize,
            None,
        )
    }

    fn initialize_client(
        audio_client: &mut AudioClient,
        desired_format: &WaveFormat,
        buffer_duration_hns: i64,
    ) -> Result<(), String> {
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns,
        };
        audio_client
            .initialize_client(desired_format, &Direction::Capture, &mode)
            .map_err(|error| format!("failed to initialize WASAPI loopback stream: {error}"))
    }

    fn drain_packets(
        capture_client: &AudioCaptureClient,
        blockalign: usize,
        sample_queue: &mut VecDeque<u8>,
        shared: &CaptureShared,
    ) -> Result<(), String> {
        loop {
            let frames = capture_client
                .get_next_packet_size()
                .map_err(|error| format!("failed to read WASAPI packet size: {error}"))?
                .unwrap_or(0);
            if frames == 0 {
                break;
            }

            let additional = (frames as usize * blockalign)
                .saturating_sub(sample_queue.capacity().saturating_sub(sample_queue.len()));
            sample_queue.reserve(additional);
            capture_client
                .read_from_device_to_deque(sample_queue)
                .map_err(|error| format!("failed to read WASAPI samples: {error}"))?;
            drain_sample_queue(blockalign, sample_queue, shared);
        }
        Ok(())
    }

    fn drain_sample_queue(
        blockalign: usize,
        sample_queue: &mut VecDeque<u8>,
        shared: &CaptureShared,
    ) {
        let chunk_bytes = blockalign.saturating_mul(CAPTURE_CHUNK_FRAMES);
        if chunk_bytes == 0 {
            return;
        }

        while sample_queue.len() >= chunk_bytes {
            let mut bytes = vec![0u8; chunk_bytes];
            for byte in &mut bytes {
                *byte = sample_queue
                    .pop_front()
                    .expect("queue length checked before pop");
            }

            let samples = float_stereo_to_mono_i16(&bytes);
            let chunk_duration_ms = duration_ms(samples.len(), CAPTURE_SAMPLE_RATE) as i64;
            let end_unix_ms = unix_ms_now();
            let start_unix_ms = end_unix_ms.saturating_sub(chunk_duration_ms);
            shared.buffer.lock().push_chunk(AudioChunk {
                start_unix_ms,
                end_unix_ms,
                samples,
            });
        }
    }

    fn float_stereo_to_mono_i16(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(8)
            .map(|frame| {
                let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                let mono = ((left + right) * 0.5).clamp(-1.0, 1.0);
                (mono * i16::MAX as f32) as i16
            })
            .collect()
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct VadRules {
    pub trailing_silence_ms: u64,
    pub no_speech_timeout_ms: u64,
    pub max_duration_ms: u64,
    pub speech_start_consecutive_frames: u8,
}

impl From<&AudioConfig> for VadRules {
    fn from(config: &AudioConfig) -> Self {
        Self {
            trailing_silence_ms: config.trailing_silence_ms,
            no_speech_timeout_ms: config.no_speech_timeout_ms,
            max_duration_ms: config.max_duration_ms,
            speech_start_consecutive_frames: 3,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct VadSegmenter {
    rules: VadRules,
    elapsed_ms: u64,
    consecutive_voiced: u8,
    speech_detected: bool,
    last_voiced_ms: Option<u64>,
    finalized: bool,
}

#[allow(dead_code)]
impl VadSegmenter {
    pub fn new(rules: VadRules) -> Self {
        Self {
            rules,
            elapsed_ms: 0,
            consecutive_voiced: 0,
            speech_detected: false,
            last_voiced_ms: None,
            finalized: false,
        }
    }

    pub fn push_frame(&mut self, voiced: bool, frame_ms: u64) -> Option<AudioEndReason> {
        if self.finalized {
            return None;
        }

        self.elapsed_ms = self.elapsed_ms.saturating_add(frame_ms);
        if voiced {
            self.consecutive_voiced = self.consecutive_voiced.saturating_add(1);
            self.last_voiced_ms = Some(self.elapsed_ms);
            if self.consecutive_voiced >= self.rules.speech_start_consecutive_frames {
                self.speech_detected = true;
            }
        } else {
            self.consecutive_voiced = 0;
        }

        let reason = if self.speech_detected {
            self.last_voiced_ms.and_then(|last| {
                (self.elapsed_ms.saturating_sub(last) >= self.rules.trailing_silence_ms)
                    .then_some(AudioEndReason::Silence)
            })
        } else if self.elapsed_ms >= self.rules.no_speech_timeout_ms {
            Some(AudioEndReason::NoSpeechTimeout)
        } else if self.elapsed_ms >= self.rules.max_duration_ms {
            Some(AudioEndReason::MaxDuration)
        } else {
            None
        };

        if reason.is_some() {
            self.finalized = true;
        }
        reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> VadRules {
        VadRules {
            trailing_silence_ms: 600,
            no_speech_timeout_ms: 5_000,
            max_duration_ms: 120_000,
            speech_start_consecutive_frames: 3,
        }
    }

    #[test]
    fn vad_detects_trailing_silence_after_speech() {
        let mut vad = VadSegmenter::new(rules());
        assert_eq!(vad.push_frame(true, 20), None);
        assert_eq!(vad.push_frame(true, 20), None);
        assert_eq!(vad.push_frame(true, 20), None);
        for _ in 0..29 {
            assert_eq!(vad.push_frame(false, 20), None);
        }
        assert_eq!(vad.push_frame(false, 20), Some(AudioEndReason::Silence));
    }

    #[test]
    fn vad_detects_no_speech_timeout() {
        let mut vad = VadSegmenter::new(rules());
        for _ in 0..249 {
            assert_eq!(vad.push_frame(false, 20), None);
        }
        assert_eq!(
            vad.push_frame(false, 20),
            Some(AudioEndReason::NoSpeechTimeout)
        );
    }

    #[test]
    fn rolling_buffer_collects_overlapping_range() {
        let mut buffer = RollingAudioBuffer::new(5_000);
        buffer.push_chunk(AudioChunk {
            start_unix_ms: 1_000,
            end_unix_ms: 2_000,
            samples: vec![1; 1_000],
        });
        buffer.push_chunk(AudioChunk {
            start_unix_ms: 2_000,
            end_unix_ms: 3_000,
            samples: vec![2; 1_000],
        });

        let samples = buffer.collect_range(1_500, 2_500);
        assert_eq!(samples.len(), 1_000);
        assert!(samples[..500].iter().all(|sample| *sample == 1));
        assert!(samples[500..].iter().all(|sample| *sample == 2));
    }

    #[test]
    fn trim_to_activity_keeps_padding_and_rejects_silence() {
        let config = AudioConfig::default();
        assert!(trim_to_activity(
            &vec![0; 4_800],
            CAPTURE_SAMPLE_RATE,
            config.activity_threshold,
            config.min_activity_ms,
            config.trim_padding_ms,
        )
        .is_none());

        let mut samples = vec![0; 100_000];
        for sample in samples.iter_mut().take(52_000).skip(50_000) {
            *sample = 1_000;
        }
        let trimmed = trim_to_activity(
            &samples,
            CAPTURE_SAMPLE_RATE,
            config.activity_threshold,
            config.min_activity_ms,
            config.trim_padding_ms,
        )
        .unwrap();
        assert_eq!(trimmed.samples.len(), 98_000);
        assert_eq!(trimmed.start_ms, 41);
        assert_eq!(trimmed.end_ms, 2083);
        assert!(trimmed.samples.len() < samples.len());
    }

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
