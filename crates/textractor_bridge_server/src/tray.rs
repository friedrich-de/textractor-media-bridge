use anyhow::{anyhow, Context, Result};
use std::{
    mem::size_of,
    net::SocketAddr,
    ptr::{copy_nonoverlapping, null_mut},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tracing::{info, warn};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use windows_sys::Win32::{
    Foundation::HGLOBAL,
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        Ole::CF_UNICODETEXT,
    },
    UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    },
};

use crate::{
    local_lan_url, localhost_endpoint_label, open_browser, run_server_thread, PreparedServer,
};

const ICON_BYTES: &[u8] = include_bytes!("../../../web_ui/public/favicon.png");
const OPEN_UI_ID: &str = "open_web_ui";
const COPY_LAN_URL_ID: &str = "copy_lan_url";
const QUIT_ID: &str = "quit";
const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

enum TrayCommand {
    OpenUi,
    CopyLanUrl,
    Quit,
}

struct TrayApp {
    _tray_icon: TrayIcon,
    _open_ui: MenuItem,
    _copy_lan_url: MenuItem,
    _quit: MenuItem,
}

pub(crate) fn run(prepared: PreparedServer) -> Result<()> {
    let local_url = prepared.local_url.clone();
    let bind_addr = prepared.bind_addr;
    let tooltip = tray_tooltip(&prepared);
    let tray_app = TrayApp::new(&tooltip)?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_handle = thread::spawn(move || run_server_thread(prepared, shutdown_rx));

    run_message_loop(tray_app, local_url, bind_addr, shutdown_tx, server_handle)
}

impl TrayApp {
    fn new(tooltip: &str) -> Result<Self> {
        let menu = Menu::new();
        let open_ui = MenuItem::with_id(OPEN_UI_ID, "Open Web UI", true, None);
        let copy_lan_url = MenuItem::with_id(COPY_LAN_URL_ID, "Copy Local LAN URL", true, None);
        let quit = MenuItem::with_id(QUIT_ID, "Quit", true, None);
        menu.append_items(&[&open_ui, &copy_lan_url, &quit])
            .context("failed to build tray menu")?;

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip(tooltip)
            .with_icon(load_icon()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .context("failed to create tray icon")?;

        Ok(Self {
            _tray_icon: tray_icon,
            _open_ui: open_ui,
            _copy_lan_url: copy_lan_url,
            _quit: quit,
        })
    }
}

fn tray_tooltip(prepared: &PreparedServer) -> String {
    let http = format!("UI {}", localhost_endpoint_label(prepared.bind_addr));
    let websocket = prepared
        .websocket_bind_addr
        .map(|addr| format!("WS {}", localhost_endpoint_label(addr)))
        .unwrap_or_else(|| "WebSocket unavailable".to_owned());
    format!("{http}\n{websocket}")
}

fn load_icon() -> Result<Icon> {
    let icon = image::load_from_memory(ICON_BYTES)
        .context("failed to decode embedded tray icon")?
        .into_rgba8();
    let (width, height) = icon.dimensions();
    Icon::from_rgba(icon.into_raw(), width, height).context("failed to create tray icon")
}

fn run_message_loop(
    tray_app: TrayApp,
    local_url: String,
    bind_addr: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
    server_handle: JoinHandle<Result<()>>,
) -> Result<()> {
    let mut tray_app = Some(tray_app);
    let mut shutdown_tx = Some(shutdown_tx);
    let mut quit_requested = false;
    let mut server_finished = false;
    let mut shutdown_started_at = None;

    while !server_finished {
        let window_quit_requested = process_windows_messages();
        if window_quit_requested && !quit_requested {
            info!("tray window quit requested");
            quit_requested = true;
            begin_tray_shutdown(&mut tray_app, &mut shutdown_tx);
            shutdown_started_at = Some(Instant::now());
        }

        while let Some(command) = next_tray_command() {
            match command {
                TrayCommand::OpenUi if !quit_requested => open_browser(&local_url),
                TrayCommand::CopyLanUrl if !quit_requested => copy_lan_url(bind_addr),
                TrayCommand::Quit if !quit_requested => {
                    info!("tray quit requested");
                    quit_requested = true;
                    begin_tray_shutdown(&mut tray_app, &mut shutdown_tx);
                    shutdown_started_at = Some(Instant::now());
                    break;
                }
                _ => {}
            }
        }

        server_finished = server_handle.is_finished();
        if quit_requested
            && !server_finished
            && shutdown_started_at
                .is_some_and(|started_at| started_at.elapsed() >= SERVER_SHUTDOWN_TIMEOUT)
        {
            warn!(
                timeout_ms = SERVER_SHUTDOWN_TIMEOUT.as_millis(),
                "server shutdown timed out; exiting tray process"
            );
            return Ok(());
        }

        if !server_finished {
            thread::sleep(Duration::from_millis(50));
        }
    }

    drop_tray_icon(&mut tray_app);

    let result = join_server(server_handle)?;
    match &result {
        Ok(()) => info!("server shutdown completed"),
        Err(error) => warn!(%error, "server shutdown failed"),
    }

    if !quit_requested {
        return match result {
            Ok(()) => Err(anyhow!("server stopped while tray mode was active")),
            Err(error) => Err(error.context("server stopped while tray mode was active")),
        };
    }
    result
}

fn begin_tray_shutdown(
    tray_app: &mut Option<TrayApp>,
    shutdown_tx: &mut Option<oneshot::Sender<()>>,
) {
    drop_tray_icon(tray_app);
    request_server_shutdown(shutdown_tx);
    info!("waiting for server shutdown");
}

fn drop_tray_icon(tray_app: &mut Option<TrayApp>) {
    if tray_app.is_some() {
        info!("dropping tray icon");
        *tray_app = None;
    }
}

fn request_server_shutdown(shutdown_tx: &mut Option<oneshot::Sender<()>>) {
    if let Some(shutdown_tx) = shutdown_tx.take() {
        info!("server shutdown requested");
        let _ = shutdown_tx.send(());
    }
}

fn process_windows_messages() -> bool {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        if msg.message == WM_QUIT {
            return true;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    false
}

fn next_tray_command() -> Option<TrayCommand> {
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        info!(id = %event.id.as_ref(), "tray menu event received");
        match event.id.as_ref() {
            OPEN_UI_ID => return Some(TrayCommand::OpenUi),
            COPY_LAN_URL_ID => return Some(TrayCommand::CopyLanUrl),
            QUIT_ID => return Some(TrayCommand::Quit),
            _ => {}
        }
    }

    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        let should_open = matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        );
        if should_open {
            info!("tray open event received");
            return Some(TrayCommand::OpenUi);
        }
    }

    None
}

fn copy_lan_url(bind_addr: SocketAddr) {
    let Some(url) = local_lan_url(bind_addr) else {
        warn!(%bind_addr, "failed to resolve a local LAN URL to copy");
        return;
    };
    match copy_text_to_clipboard(&url) {
        Ok(()) => info!(%url, "copied local LAN URL to clipboard"),
        Err(error) => warn!(%error, %url, "failed to copy local LAN URL to clipboard"),
    }
}

fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = encoded.len() * size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle.is_null() {
            return Err(anyhow!("failed to allocate clipboard memory"));
        }

        let data = GlobalLock(handle);
        if data.is_null() {
            global_free(handle);
            return Err(anyhow!("failed to lock clipboard memory"));
        }

        copy_nonoverlapping(encoded.as_ptr(), data.cast::<u16>(), encoded.len());
        GlobalUnlock(handle);

        if OpenClipboard(null_mut()) == 0 {
            global_free(handle);
            return Err(anyhow!("failed to open clipboard"));
        }
        let _guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            global_free(handle);
            return Err(anyhow!("failed to empty clipboard"));
        }

        if SetClipboardData(u32::from(CF_UNICODETEXT), handle).is_null() {
            global_free(handle);
            return Err(anyhow!("failed to set clipboard data"));
        }
    }

    Ok(())
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

unsafe fn global_free(handle: HGLOBAL) {
    unsafe extern "system" {
        fn GlobalFree(hmem: HGLOBAL) -> HGLOBAL;
    }

    let _ = unsafe { GlobalFree(handle) };
}

fn join_server(server_handle: JoinHandle<Result<()>>) -> Result<Result<()>> {
    server_handle
        .join()
        .map_err(|_| anyhow!("server thread panicked"))
}
