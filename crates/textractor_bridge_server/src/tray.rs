use anyhow::{anyhow, Context, Result};
use std::{
    mem::size_of,
    net::SocketAddr,
    ptr::{copy_nonoverlapping, null_mut},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
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

use crate::{local_lan_url, open_browser, run_server_thread, PreparedServer};

const ICON_BYTES: &[u8] = include_bytes!("../../../web_ui/public/favicon.png");
const OPEN_UI_ID: &str = "open_web_ui";
const COPY_LAN_URL_ID: &str = "copy_lan_url";
const QUIT_ID: &str = "quit";

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
    let (command_tx, command_rx) = mpsc::channel();
    let _tray_app = TrayApp::new(command_tx)?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_handle = thread::spawn(move || run_server_thread(prepared, shutdown_rx));

    run_message_loop(command_rx, local_url, bind_addr, shutdown_tx, server_handle)
}

impl TrayApp {
    fn new(command_tx: Sender<TrayCommand>) -> Result<Self> {
        let menu = Menu::new();
        let open_ui = MenuItem::with_id(OPEN_UI_ID, "Open Web UI", true, None);
        let copy_lan_url = MenuItem::with_id(COPY_LAN_URL_ID, "Copy Local LAN URL", true, None);
        let quit = MenuItem::with_id(QUIT_ID, "Quit", true, None);
        menu.append_items(&[&open_ui, &copy_lan_url, &quit])
            .context("failed to build tray menu")?;

        let menu_tx = command_tx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match event.id.as_ref() {
                OPEN_UI_ID => Some(TrayCommand::OpenUi),
                COPY_LAN_URL_ID => Some(TrayCommand::CopyLanUrl),
                QUIT_ID => Some(TrayCommand::Quit),
                _ => None,
            };
            if let Some(command) = command {
                let _ = menu_tx.send(command);
            }
        }));

        TrayIconEvent::set_event_handler(Some(move |event| {
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
                let _ = command_tx.send(TrayCommand::OpenUi);
            }
        }));

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Textractor Media Bridge")
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

fn load_icon() -> Result<Icon> {
    let icon = image::load_from_memory(ICON_BYTES)
        .context("failed to decode embedded tray icon")?
        .into_rgba8();
    let (width, height) = icon.dimensions();
    Icon::from_rgba(icon.into_raw(), width, height).context("failed to create tray icon")
}

fn run_message_loop(
    command_rx: Receiver<TrayCommand>,
    local_url: String,
    bind_addr: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
    server_handle: JoinHandle<Result<()>>,
) -> Result<()> {
    let mut shutdown_tx = Some(shutdown_tx);
    let mut quit_requested = false;
    let mut server_finished = false;

    while !quit_requested && !server_finished {
        quit_requested = process_windows_messages();
        while let Ok(command) = command_rx.try_recv() {
            match command {
                TrayCommand::OpenUi => open_browser(&local_url),
                TrayCommand::CopyLanUrl => copy_lan_url(bind_addr),
                TrayCommand::Quit => {
                    quit_requested = true;
                    break;
                }
            }
        }

        server_finished = server_handle.is_finished();
        if !quit_requested && !server_finished {
            thread::sleep(Duration::from_millis(50));
        }
    }

    if !server_finished {
        if let Some(shutdown_tx) = shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }

    let result = join_server(server_handle)?;
    if server_finished && !quit_requested {
        return match result {
            Ok(()) => Err(anyhow!("server stopped while tray mode was active")),
            Err(error) => Err(error.context("server stopped while tray mode was active")),
        };
    }
    result
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
