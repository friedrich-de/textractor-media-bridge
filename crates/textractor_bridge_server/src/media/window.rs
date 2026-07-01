pub type NativeHwnd = isize;

#[derive(Debug, Clone, Copy)]
pub struct WindowCandidate {
    pub hwnd: NativeHwnd,
    pub area: i64,
    pub foreground: bool,
    pub root_owner: bool,
}

pub fn resolve_process_window(process_id: u32) -> Option<NativeHwnd> {
    platform_resolve_process_window(process_id)
}

pub fn resolve_process_window_title(process_id: u32) -> Option<String> {
    resolve_process_window(process_id).and_then(platform_window_title)
}

#[cfg(windows)]
fn platform_resolve_process_window(process_id: u32) -> Option<NativeHwnd> {
    use std::{ffi::c_void, mem};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
        UI::WindowsAndMessaging::{
            EnumWindows, GetAncestor, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
            IsWindowVisible, GA_ROOTOWNER,
        },
    };

    struct EnumContext {
        process_id: u32,
        foreground: HWND,
        candidates: Vec<WindowCandidate>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        let context = unsafe { &mut *(lparam as *mut EnumContext) };

        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }

        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid != context.process_id {
            return 1;
        }

        let mut cloaked = 0u32;
        let cloaked_ok = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast::<c_void>(),
                mem::size_of::<u32>() as u32,
            )
        } == 0;
        if cloaked_ok && cloaked != 0 {
            return 1;
        }

        let rect = unsafe { extended_frame_rect(hwnd) }.or_else(|| unsafe { window_rect(hwnd) });
        let Some(rect) = rect else {
            return 1;
        };
        let width = i64::from(rect.right - rect.left);
        let height = i64::from(rect.bottom - rect.top);
        if width <= 0 || height <= 0 {
            return 1;
        }

        let root = unsafe { GetAncestor(hwnd, GA_ROOTOWNER) };
        context.candidates.push(WindowCandidate {
            hwnd: hwnd as NativeHwnd,
            area: width.saturating_mul(height),
            foreground: hwnd == context.foreground,
            root_owner: root == hwnd,
        });

        1
    }

    unsafe fn extended_frame_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = unsafe { mem::zeroed::<RECT>() };
        let ok = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                (&mut rect as *mut RECT).cast::<c_void>(),
                mem::size_of::<RECT>() as u32,
            )
        } == 0;
        ok.then_some(rect)
    }

    unsafe fn window_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = unsafe { mem::zeroed::<RECT>() };
        (unsafe { GetWindowRect(hwnd, &mut rect) } != 0).then_some(rect)
    }

    unsafe {
        let mut context = EnumContext {
            process_id,
            foreground: GetForegroundWindow(),
            candidates: Vec::new(),
        };

        EnumWindows(
            Some(enum_proc),
            (&mut context as *mut EnumContext) as LPARAM,
        );
        context.candidates.sort_by(|a, b| {
            b.foreground
                .cmp(&a.foreground)
                .then_with(|| b.root_owner.cmp(&a.root_owner))
                .then_with(|| b.area.cmp(&a.area))
        });
        context.candidates.first().map(|candidate| candidate.hwnd)
    }
}

#[cfg(windows)]
fn platform_window_title(hwnd: NativeHwnd) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW},
    };

    unsafe {
        let hwnd = hwnd as HWND;
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }

        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if copied <= 0 {
            return None;
        }

        let title = String::from_utf16_lossy(&buffer[..copied as usize])
            .trim()
            .to_owned();
        (!title.is_empty()).then_some(title)
    }
}

#[cfg(not(windows))]
fn platform_resolve_process_window(_process_id: u32) -> Option<NativeHwnd> {
    None
}

#[cfg(not(windows))]
fn platform_window_title(_hwnd: NativeHwnd) -> Option<String> {
    None
}
