pub const PIPE_PREFIX: &str = r"\\.\pipe\textractor_media_bridge";
pub const PIPE_PROTOCOL_SUFFIX: &str = "v1";

pub fn pipe_name_from_sid(sid: &str) -> String {
    let sanitized: String = sid
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    format!("{PIPE_PREFIX}_{sanitized}_{PIPE_PROTOCOL_SUFFIX}")
}

pub fn default_pipe_name() -> String {
    let sid = current_user_sid_string().unwrap_or_else(|| "unknown_user".to_owned());
    pipe_name_from_sid(&sid)
}

#[cfg(windows)]
pub fn current_user_sid_string() -> Option<String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }

        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }

        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr().cast(),
            needed,
            &mut needed,
        );
        if ok == 0 {
            let _ = CloseHandle(token);
            return None;
        }

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_ptr = ptr::null_mut();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_ptr) == 0 {
            let _ = CloseHandle(token);
            return None;
        }

        let mut len = 0usize;
        while *sid_ptr.add(len) != 0 {
            len += 1;
        }
        let sid = OsString::from_wide(std::slice::from_raw_parts(sid_ptr, len))
            .to_string_lossy()
            .into_owned();
        let _ = LocalFree(sid_ptr.cast());
        let _ = CloseHandle(token);
        Some(sid)
    }
}

#[cfg(not(windows))]
pub fn current_user_sid_string() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_sanitizes_sid() {
        assert_eq!(
            pipe_name_from_sid("S-1-5-21 bad/user"),
            r"\\.\pipe\textractor_media_bridge_S-1-5-21baduser_v1"
        );
    }
}
