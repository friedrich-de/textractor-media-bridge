use std::{ffi::CStr, os::raw::c_char};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InfoForExtension {
    pub name: *const c_char,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSentenceInfo {
    pub current_select: i64,
    pub process_id: u32,
    pub text_number: i64,
    pub text_name: Option<String>,
}

pub fn parse_sentence_info(info: *const InfoForExtension) -> Option<ParsedSentenceInfo> {
    if info.is_null() {
        return None;
    }

    let mut parsed = ParsedSentenceInfo {
        current_select: 0,
        process_id: 0,
        text_number: 0,
        text_name: None,
    };

    unsafe {
        for index in 0..128usize {
            let entry = info.add(index);
            let name_ptr = (*entry).name;
            if name_ptr.is_null() {
                break;
            }

            let Ok(name) = CStr::from_ptr(name_ptr).to_str() else {
                continue;
            };
            let value = (*entry).value;

            match name {
                "current select" => parsed.current_select = value,
                "process id" => {
                    parsed.process_id = u32::try_from(value).unwrap_or(0);
                }
                "text number" => parsed.text_number = value,
                "text name" => {
                    parsed.text_name = try_read_text_name(value);
                }
                _ => {}
            }
        }
    }

    Some(parsed)
}

fn try_read_text_name(value: i64) -> Option<String> {
    if value <= 0x10000 {
        return None;
    }
    let ptr = value as usize as *const c_char;
    if !is_probably_readable_string(ptr) {
        return None;
    }

    unsafe {
        CStr::from_ptr(ptr)
            .to_str()
            .ok()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
    }
}

#[cfg(windows)]
fn is_probably_readable_string(ptr: *const c_char) -> bool {
    use std::{ffi::c_void, mem};
    use windows_sys::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
    };

    if ptr.is_null() {
        return false;
    }

    unsafe {
        let mut mbi = mem::zeroed::<MEMORY_BASIC_INFORMATION>();
        let result = VirtualQuery(
            ptr.cast::<c_void>(),
            &mut mbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );
        result != 0
            && mbi.State == MEM_COMMIT
            && (mbi.Protect & PAGE_NOACCESS) == 0
            && (mbi.Protect & PAGE_GUARD) == 0
    }
}

#[cfg(not(windows))]
fn is_probably_readable_string(ptr: *const c_char) -> bool {
    !ptr.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn parses_known_sentence_info_fields() {
        let current = CString::new("current select").unwrap();
        let process = CString::new("process id").unwrap();
        let number = CString::new("text number").unwrap();
        let name = CString::new("text name").unwrap();
        let hook_name = CString::new("hook: dialog").unwrap();
        let entries = [
            InfoForExtension {
                name: current.as_ptr(),
                value: 1,
            },
            InfoForExtension {
                name: process.as_ptr(),
                value: 1234,
            },
            InfoForExtension {
                name: number.as_ptr(),
                value: 17,
            },
            InfoForExtension {
                name: name.as_ptr(),
                value: hook_name.as_ptr() as isize as i64,
            },
            InfoForExtension {
                name: std::ptr::null(),
                value: 0,
            },
        ];

        let parsed = parse_sentence_info(entries.as_ptr()).expect("parsed");
        assert_eq!(parsed.current_select, 1);
        assert_eq!(parsed.process_id, 1234);
        assert_eq!(parsed.text_number, 17);
        assert_eq!(parsed.text_name.as_deref(), Some("hook: dialog"));
    }
}
