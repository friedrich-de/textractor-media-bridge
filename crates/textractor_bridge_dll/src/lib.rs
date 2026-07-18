#![allow(clippy::missing_safety_doc)]

use bridge_protocol::{default_pipe_name, write_frame, PipeLineEvent, PipeLineMeta};
use sentence_info::{parse_sentence_info, ParsedSentenceInfo};
#[cfg(windows)]
use std::io::Write;
use std::{
    fs::{File, OpenOptions},
    panic::catch_unwind,
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod sentence_info;

pub use sentence_info::InfoForExtension;

const BOOTSTRAP_BACKOFF_MS: i64 = 5_000;

static MESSAGE_ID: AtomicU64 = AtomicU64::new(1);
static LAST_BOOTSTRAP_ATTEMPT_MS: AtomicI64 = AtomicI64::new(0);

#[no_mangle]
pub extern "C" fn OnNewSentence(
    sentence: *const u16,
    sentence_info: *const InfoForExtension,
) -> *const u16 {
    let _ = catch_unwind(|| {
        handle_new_sentence(sentence, sentence_info);
    });
    sentence
}

fn handle_new_sentence(sentence: *const u16, sentence_info: *const InfoForExtension) {
    let Some(info) = parse_sentence_info(sentence_info) else {
        return;
    };
    if !should_forward(&info) {
        return;
    }

    let text = utf16_ptr_to_string_lossy(sentence);
    if text.trim().is_empty() {
        return;
    }

    let meta = PipeLineMeta {
        process_id: info.process_id,
        thread_number: info.text_number,
        thread_name: None,
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

    send_event_to_server(event);
}

fn should_forward(info: &ParsedSentenceInfo) -> bool {
    info.current_select != 0 && info.text_number != 0 && info.text_number != 1
}

fn send_event_to_server(event: PipeLineEvent) {
    let pipe_name = default_pipe_name();
    let Some(mut pipe) = connect_pipe(&pipe_name) else {
        maybe_bootstrap_server("pipe open failed");
        return;
    };

    if write_frame(&mut pipe, &event).is_err() {
        maybe_bootstrap_server("pipe write failed");
    }
}

fn connect_pipe(pipe_name: &str) -> Option<File> {
    OpenOptions::new().write(true).open(pipe_name).ok()
}

fn maybe_bootstrap_server(reason: &str) {
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

    platform_bootstrap_server(reason);
}

#[cfg(windows)]
fn platform_bootstrap_server(reason: &str) {
    use std::{
        ffi::OsStr,
        os::windows::{ffi::OsStrExt, process::CommandExt},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::{CreateMutexW, CREATE_NO_WINDOW},
    };

    let exe = server_exe_path();
    let server_dir = exe.parent().map(Path::to_path_buf);
    let config = server_dir
        .as_ref()
        .map(|dir| dir.join("config").join("bridge.toml"))
        .filter(|path| path.is_file());

    append_bootstrap_log(
        server_dir.as_deref(),
        &format!(
            "bootstrap requested; reason={}; current_exe={}; current_dir={}; server_exe={}; server_exists={}; config={}",
            reason,
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<error: {error}>")),
            std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("<error: {error}>")),
            exe.display(),
            exe.is_file(),
            config
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_owned()),
        ),
    );
    remove_legacy_launcher_script(server_dir.as_deref());

    let mutex_name: Vec<u16> = OsStr::new("Local\\TextractorMediaBridgeServerBootstrap_v1")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mutex = CreateMutexW(ptr::null(), 0, mutex_name.as_ptr());
        if mutex.is_null() {
            append_bootstrap_log(
                server_dir.as_deref(),
                &format!("CreateMutexW failed; error={}", GetLastError()),
            );
            return;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            append_bootstrap_log(
                server_dir.as_deref(),
                "bootstrap mutex already exists; skipping",
            );
            let _ = CloseHandle(mutex);
            return;
        }

        spawn_server_directly(&exe, server_dir.as_deref(), config.as_deref());

        let _ = CloseHandle(mutex);
    }

    fn server_exe_path() -> PathBuf {
        if let Some(path) = std::env::var_os("TEXTRACTOR_MEDIA_BRIDGE_SERVER_EXE") {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                return path;
            }
            if let Some(dir) = textractor_dir() {
                return dir.join(path);
            }
            return path;
        }

        textractor_dir()
            .map(|dir| dir.join("textractor_bridge_server.exe"))
            .unwrap_or_else(|| PathBuf::from("textractor_bridge_server.exe"))
    }

    fn textractor_dir() -> Option<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }

    fn spawn_server_directly(exe: &Path, server_dir: Option<&Path>, config: Option<&Path>) {
        let mut command = Command::new(exe);
        if let Some(dir) = server_dir {
            command.current_dir(dir);
        }
        if let Some(config) = config {
            command.arg("--config").arg(config);
        }

        let result = command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(log_stdio(
                server_dir,
                "textractor_bridge_server.autostart.stdout.log",
            ))
            .stderr(log_stdio(
                server_dir,
                "textractor_bridge_server.autostart.stderr.log",
            ))
            .spawn();

        match result {
            Ok(child) => append_bootstrap_log(
                server_dir,
                &format!("spawned server directly; pid={}", child.id()),
            ),
            Err(error) => append_bootstrap_log(
                server_dir,
                &format!(
                    "direct server spawn failed; error={}; raw_os_error={:?}",
                    error,
                    error.raw_os_error()
                ),
            ),
        }
    }

    fn remove_legacy_launcher_script(server_dir: Option<&Path>) {
        let Some(server_dir) = server_dir else {
            return;
        };
        let script = server_dir.join("textractor_bridge_server.autostart.cmd");
        match std::fs::remove_file(&script) {
            Ok(()) => append_bootstrap_log(
                Some(server_dir),
                &format!("removed legacy launcher script {}", script.display()),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => append_bootstrap_log(
                Some(server_dir),
                &format!(
                    "failed to remove legacy launcher script {}; error={}",
                    script.display(),
                    error
                ),
            ),
        }
    }

    fn log_stdio(server_dir: Option<&Path>, name: &str) -> Stdio {
        server_dir
            .and_then(|dir| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(name))
                    .ok()
            })
            .map(Stdio::from)
            .unwrap_or_else(Stdio::null)
    }

    fn append_bootstrap_log(server_dir: Option<&Path>, message: &str) {
        let path = server_dir
            .map(|dir| dir.join("textractor_bridge_dll.bootstrap.log"))
            .unwrap_or_else(|| PathBuf::from("textractor_bridge_dll.bootstrap.log"));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "[{}] {message}", unix_ms_now());
        }
    }
}

#[cfg(not(windows))]
fn platform_bootstrap_server(_reason: &str) {}

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
        };
        assert!(should_forward(&info));
    }

    #[test]
    fn filter_rejects_unselected_console_and_clipboard() {
        let mut info = ParsedSentenceInfo {
            current_select: 0,
            process_id: 12,
            text_number: 2,
        };
        assert!(!should_forward(&info));
        info.current_select = 1;
        info.text_number = 0;
        assert!(!should_forward(&info));
        info.text_number = 1;
        assert!(!should_forward(&info));
    }
}
