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
}

pub fn parse_sentence_info(info: *const InfoForExtension) -> Option<ParsedSentenceInfo> {
    if info.is_null() {
        return None;
    }

    let mut parsed = ParsedSentenceInfo {
        current_select: 0,
        process_id: 0,
        text_number: 0,
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
                _ => {}
            }
        }
    }

    Some(parsed)
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
                name: std::ptr::null(),
                value: 0,
            },
        ];

        let parsed = parse_sentence_info(entries.as_ptr()).expect("parsed");
        assert_eq!(parsed.current_select, 1);
        assert_eq!(parsed.process_id, 1234);
        assert_eq!(parsed.text_number, 17);
    }
}
