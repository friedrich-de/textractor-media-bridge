#![allow(clippy::missing_safety_doc)]

use bridge_protocol::{default_pipe_name, write_frame, PipeLineEvent, PipeLineMeta};
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use sentence_info::{parse_sentence_info, ParsedSentenceInfo};
use std::{
    fs::{File, OpenOptions},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        OnceLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod sentence_info;

pub use sentence_info::InfoForExtension;

const QUEUE_CAPACITY: usize = 256;
const BOOTSTRAP_BACKOFF_MS: i64 = 5_000;

static MESSAGE_ID: AtomicU64 = AtomicU64::new(1);
static QUEUE: OnceLock<Sender<PipeLineEvent>> = OnceLock::new();
static LAST_BOOTSTRAP_ATTEMPT_MS: AtomicI64 = AtomicI64::new(0);

#[no_mangle]
pub extern "C" fn OnNewSentence(
    sentence: *const u16,
    sentence_info: *const InfoForExtension,
) -> *const u16 {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        handle_new_sentence(sentence, sentence_info);
    }));
    sentence
}

fn handle_new_sentence(sentence: *const u16, sentence_info: *const InfoForExtension) {
    let Some(info) = parse_sentence_info(sentence_info) else {
        return;
    };
    if !should_forward(&info) {
        return;
    }

    let text = repair_utf8_mojibake(utf16_ptr_to_string_lossy(sentence));
    if text.trim().is_empty() {
        return;
    }

    let meta = PipeLineMeta {
        process_id: info.process_id,
        thread_number: info.text_number,
        thread_name: info.text_name,
        window_title: None,
        is_current_select: info.current_select != 0,
        arch: target_arch(),
        source: "textractor".to_owned(),
    };
    let event = PipeLineEvent::new(
        MESSAGE_ID.fetch_add(1, Ordering::Relaxed),
        unix_ms_now(),
        text,
        meta,
    );

    match sender().try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn should_forward(info: &ParsedSentenceInfo) -> bool {
    info.current_select != 0 && info.text_number != 0 && info.text_number != 1
}

fn sender() -> &'static Sender<PipeLineEvent> {
    QUEUE.get_or_init(|| {
        let (tx, rx) = bounded(QUEUE_CAPACITY);
        let _ = thread::Builder::new()
            .name("textractor-media-bridge-pipe".to_owned())
            .spawn(move || pipe_worker(rx));
        tx
    })
}

fn pipe_worker(rx: Receiver<PipeLineEvent>) {
    let pipe_name = default_pipe_name();
    let mut pipe: Option<File> = None;

    while let Ok(event) = rx.recv() {
        if pipe.is_none() {
            pipe = connect_pipe(&pipe_name);
        }

        match pipe.as_mut() {
            Some(file) => {
                if write_frame(file, &event).is_err() {
                    pipe = None;
                    maybe_bootstrap_server();
                }
            }
            None => {
                maybe_bootstrap_server();
            }
        }

        while let Ok(next) = rx.try_recv() {
            if pipe.is_none() {
                pipe = connect_pipe(&pipe_name);
            }
            match pipe.as_mut() {
                Some(file) => {
                    if write_frame(file, &next).is_err() {
                        pipe = None;
                        break;
                    }
                }
                None => {
                    pipe = None;
                    break;
                }
            }
        }
    }
}

fn connect_pipe(pipe_name: &str) -> Option<File> {
    OpenOptions::new().write(true).open(pipe_name).ok()
}

fn maybe_bootstrap_server() {
    let now = unix_ms_now();
    let previous = LAST_BOOTSTRAP_ATTEMPT_MS.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < BOOTSTRAP_BACKOFF_MS {
        return;
    }
    if LAST_BOOTSTRAP_ATTEMPT_MS
        .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }

    platform_bootstrap_server();
}

#[cfg(windows)]
fn platform_bootstrap_server() {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, process::Command, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };

    let mutex_name: Vec<u16> = OsStr::new("Local\\TextractorMediaBridgeServerBootstrap_v1")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mutex = CreateMutexW(ptr::null(), 0, mutex_name.as_ptr());
        if mutex.is_null() {
            return;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return;
        }

        let exe = std::env::var_os("TEXTRACTOR_MEDIA_BRIDGE_SERVER_EXE")
            .unwrap_or_else(|| "textractor_bridge_server.exe".into());
        let _ = Command::new(exe)
            .arg("--open")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        let _ = CloseHandle(mutex);
    }
}

#[cfg(not(windows))]
fn platform_bootstrap_server() {}

fn utf16_ptr_to_string_lossy(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }

    const MAX_U16S: usize = 1_000_000;
    unsafe {
        let mut len = 0usize;
        while len < MAX_U16S && *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

fn repair_utf8_mojibake(text: String) -> String {
    if !looks_like_utf8_as_latin1(&text) {
        return text;
    }

    let mut bytes = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let value = ch as u32;
        if value > u8::MAX as u32 {
            return text;
        }
        bytes.push(value as u8);
    }

    let Ok(candidate) = String::from_utf8(bytes) else {
        return text;
    };

    if japanese_score(&candidate) > japanese_score(&text).saturating_add(2) {
        candidate
    } else {
        text
    }
}

fn looks_like_utf8_as_latin1(text: &str) -> bool {
    let marker_count = text
        .chars()
        .filter(|ch| {
            matches!(
                *ch as u32,
                0x00e2 | 0x00e3 | 0x00e4 | 0x00e5 | 0x00e6 | 0x00e7 | 0x00e8 | 0x00e9 | 0x00ef
            )
        })
        .count();
    let control_count = text
        .chars()
        .filter(|ch| {
            let value = *ch as u32;
            (0x80..=0x9f).contains(&value)
        })
        .count();
    marker_count >= 2 || control_count >= 2
}

fn japanese_score(text: &str) -> usize {
    text.chars()
        .filter(|ch| {
            let value = *ch as u32;
            (0x3040..=0x30ff).contains(&value)
                || (0x3400..=0x9fff).contains(&value)
                || (0xff00..=0xffef).contains(&value)
                || (0x3000..=0x303f).contains(&value)
        })
        .count()
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn target_arch() -> String {
    if cfg!(target_pointer_width = "64") {
        "x64".to_owned()
    } else {
        "x86".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_accepts_selected_real_text_threads() {
        let info = ParsedSentenceInfo {
            current_select: 1,
            process_id: 12,
            text_number: 2,
            text_name: None,
        };
        assert!(should_forward(&info));
    }

    #[test]
    fn filter_rejects_unselected_console_and_clipboard() {
        let mut info = ParsedSentenceInfo {
            current_select: 0,
            process_id: 12,
            text_number: 2,
            text_name: None,
        };
        assert!(!should_forward(&info));
        info.current_select = 1;
        info.text_number = 0;
        assert!(!should_forward(&info));
        info.text_number = 1;
        assert!(!should_forward(&info));
    }

    #[test]
    fn repairs_utf8_bytes_widened_as_latin1() {
        let expected = "\u{3000}\u{3060}\u{304b}\u{3089}\u{50d5}\u{306f}\n";
        let mojibake = expected
            .as_bytes()
            .iter()
            .map(|byte| char::from(*byte))
            .collect::<String>();
        assert_eq!(repair_utf8_mojibake(mojibake), expected);
    }

    #[test]
    fn leaves_valid_japanese_text_alone() {
        let text = "\u{3000}\u{3060}\u{304b}\u{3089}\u{50d5}\u{306f}\n";
        assert_eq!(repair_utf8_mojibake(text.to_owned()), text);
    }
}
