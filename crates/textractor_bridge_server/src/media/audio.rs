mod wav;

pub use self::wav::{encode_pcm16_wav, slice_pcm16_wav};

use self::wav::{duration_ms, ms_to_sample_index};
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
pub const MAIN_AUDIO_PREROLL_MS: u64 = 2_000;
pub const TRIM_AUDIO_PREROLL_MS: u64 = 10_000;
pub const TRIM_AUDIO_POSTROLL_MS: u64 = 10_000;
pub const MAIN_AUDIO_MAX_DURATION_MS: u64 = 120_000;
pub const TRIM_AUDIO_MAX_DURATION_MS: u64 = 180_000;

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
        let main_start_unix_ms = session
            .started_unix_ms
            .saturating_sub(ms_to_i64(MAIN_AUDIO_PREROLL_MS));
        let Some(captured) =
            self.collect_process_range(session.process_id, main_start_unix_ms, end_unix_ms)
        else {
            self.sessions.lock().remove(&line_id);
            return Some(FinishedMainAudio::NoAudio {
                reason: format!(
                    "audio capture worker for process {} was unavailable",
                    session.process_id
                ),
            });
        };

        if captured.samples.is_empty() {
            let reason =
                no_audio_reason(session.process_id, captured.samples.len(), captured.status);
            self.sessions.lock().remove(&line_id);
            return Some(FinishedMainAudio::NoAudio { reason });
        }

        let duration_ms = duration_ms(captured.samples.len(), CAPTURE_SAMPLE_RATE);
        Some(FinishedMainAudio::Ready(CapturedMainAudio {
            bytes: encode_pcm16_wav(&captured.samples, CAPTURE_SAMPLE_RATE),
            duration_ms,
            end_reason: reason,
            mime_type: "audio/wav",
            trim_recording_started_unix_ms: session
                .started_unix_ms
                .saturating_sub(ms_to_i64(TRIM_AUDIO_PREROLL_MS)),
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
        let source_start_unix_ms = session
            .started_unix_ms
            .saturating_sub(ms_to_i64(TRIM_AUDIO_PREROLL_MS));
        let captured =
            match self.collect_process_range(session.process_id, source_start_unix_ms, end_unix_ms)
            {
                Some(captured) if !captured.samples.is_empty() => captured,
                Some(_) => {
                    return Some(FinishedTrimAudio::NoAudio {
                        reason: format!(
                            "no trim audio samples captured for process {}",
                            session.process_id
                        ),
                    });
                }
                None => {
                    return Some(FinishedTrimAudio::NoAudio {
                        reason: format!(
                            "audio capture worker for process {} was unavailable",
                            session.process_id
                        ),
                    });
                }
            };

        let source_duration_ms = duration_ms(captured.samples.len(), CAPTURE_SAMPLE_RATE);
        Some(FinishedTrimAudio::Ready(CapturedTrimAudio {
            source_bytes: encode_pcm16_wav(&captured.samples, CAPTURE_SAMPLE_RATE),
            source_duration_ms,
            start_ms: 0,
            end_ms: source_duration_ms,
            mime_type: "audio/wav",
        }))
    }

    pub fn main_recording_line_ids_for_process(&self, process_id: u32) -> Vec<LineId> {
        self.sessions
            .lock()
            .iter()
            .filter_map(|(line_id, session)| {
                (session.process_id == process_id && !session.main_finished).then_some(*line_id)
            })
            .collect()
    }

    pub fn is_recording(&self, line_id: LineId) -> bool {
        self.sessions.lock().contains_key(&line_id)
    }

    pub fn is_main_recording(&self, line_id: LineId) -> bool {
        self.sessions
            .lock()
            .get(&line_id)
            .is_some_and(|session| !session.main_finished)
    }

    pub fn clear_sessions(&self) {
        self.sessions.lock().clear();
    }

    pub fn remove_line_session(&self, line_id: LineId) {
        self.sessions.lock().remove(&line_id);
    }

    #[cfg(test)]
    pub(crate) fn insert_test_samples(
        &self,
        process_id: u32,
        start_unix_ms: i64,
        samples: Vec<i16>,
    ) {
        let shared = Arc::new(CaptureShared::new(
            self.retention_ms(&AudioConfig::default()),
        ));
        shared.set_ready("test", None);
        let duration = duration_ms(samples.len(), CAPTURE_SAMPLE_RATE) as i64;
        shared.buffer.lock().push_chunk(AudioChunk {
            start_unix_ms,
            end_unix_ms: start_unix_ms.saturating_add(duration),
            samples,
        });
        self.workers
            .lock()
            .insert(process_id, CaptureWorker { shared });
    }

    fn collect_process_range(
        &self,
        process_id: u32,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> Option<CapturedRange> {
        let worker = self.workers.lock().get(&process_id).cloned()?;
        let samples = worker
            .shared
            .buffer
            .lock()
            .collect_range(start_unix_ms, end_unix_ms);
        let status = worker.shared.status.lock().clone();
        Some(CapturedRange { samples, status })
    }

    fn ensure_worker(&self, process_id: u32, config: AudioConfig) {
        let retention_ms = self.retention_ms(&config);
        let mut workers = self.workers.lock();
        workers
            .entry(process_id)
            .or_insert_with(|| CaptureWorker::spawn(process_id, config, retention_ms));
    }

    fn retention_ms(&self, _config: &AudioConfig) -> i64 {
        TRIM_AUDIO_PREROLL_MS
            .saturating_add(TRIM_AUDIO_MAX_DURATION_MS)
            .saturating_add(TRIM_AUDIO_POSTROLL_MS)
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

#[derive(Debug, Clone)]
struct CapturedRange {
    samples: Vec<i16>,
    status: CaptureStatus,
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
    "no audio samples captured".to_owned()
}

fn ms_to_i64(ms: u64) -> i64 {
    ms.min(i64::MAX as u64) as i64
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn main_audio_captures_preroll_through_end_without_activity_trim() {
        let manager = AudioManager::new(AudioConfig::default());
        manager.insert_test_samples(7, 0, samples_by_ms(20_000));

        manager.start_line_session(1, 7, 10_000);
        let finished = manager
            .finish_main_line_session_at(1, AudioEndReason::LineAdvanced, 12_500)
            .expect("main session should finish");

        let FinishedMainAudio::Ready(captured) = finished else {
            panic!("main audio should be ready");
        };
        assert_eq!(captured.duration_ms, 4_500);
        assert_eq!(captured.end_reason, AudioEndReason::LineAdvanced);
        assert_eq!(captured.trim_recording_started_unix_ms, 0);

        let samples = decode_encoded_samples(&captured.bytes);
        assert_eq!(samples.len(), 216_000);
        assert_eq!(samples.first(), Some(&8_000));
        assert_eq!(samples.last(), Some(&12_499));
    }

    #[test]
    fn trim_audio_captures_preroll_through_explicit_end() {
        let manager = AudioManager::new(AudioConfig::default());
        manager.insert_test_samples(7, 0, samples_by_ms(30_000));

        manager.start_line_session(1, 7, 11_000);
        let finished = manager
            .finish_trim_line_session_at(1, AudioEndReason::Manual, 25_000)
            .expect("trim session should finish");

        let FinishedTrimAudio::Ready(captured) = finished else {
            panic!("trim audio should be ready");
        };
        assert_eq!(captured.source_duration_ms, 24_000);
        assert_eq!(captured.start_ms, 0);
        assert_eq!(captured.end_ms, 24_000);

        let samples = decode_encoded_samples(&captured.source_bytes);
        assert_eq!(samples.len(), 1_152_000);
        assert_eq!(samples.first(), Some(&1_000));
        assert_eq!(samples.last(), Some(&24_999));
    }

    #[test]
    fn main_recording_ids_skip_sessions_waiting_for_trim_source() {
        let manager = AudioManager::new(AudioConfig::default());
        manager.insert_test_samples(7, 0, samples_by_ms(30_000));

        manager.start_line_session(1, 7, 5_000);
        manager.start_line_session(2, 7, 6_000);
        manager.finish_main_line_session_at(1, AudioEndReason::LineAdvanced, 7_000);

        assert_eq!(manager.main_recording_line_ids_for_process(7), vec![2]);
        assert!(manager.is_recording(1));
        assert!(!manager.is_main_recording(1));
    }

    fn samples_by_ms(duration_ms: u64) -> Vec<i16> {
        (0..duration_ms)
            .flat_map(|ms| std::iter::repeat(ms as i16).take(48))
            .collect()
    }

    fn decode_encoded_samples(bytes: &[u8]) -> Vec<i16> {
        bytes[44..]
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect()
    }
}
