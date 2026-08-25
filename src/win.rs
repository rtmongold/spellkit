use crate::Error;

use std::ffi::OsStr;
use std::iter;
use std::os::windows::ffi::OsStrExt;

use windows::{
    Win32::{
        Foundation::S_FALSE,
        Globalization::{
            IEnumSpellingError, ISpellChecker, ISpellCheckerFactory, ISpellingError,
            SpellCheckerFactory,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree,
        },
    },
    core::{HSTRING, PWSTR},
};

fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(iter::once(0)).collect()
}

fn utf16_offset_to_utf8(s: &str, utf16_units: usize) -> usize {
    let mut units = 0;
    for (byte_idx, ch) in s.char_indices() {
        if units >= utf16_units {
            return byte_idx;
        }
        units += ch.len_utf16();
    }
    s.len()
}

fn open_for_language(bcp47: &str) -> Result<ISpellChecker, Error> {
    // S_OK / S_FALSE (already initialized) both succeed via windows::Result
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    let factory: ISpellCheckerFactory =
        unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| Error::Unavailable)?;

    let tag = HSTRING::from(bcp47);
    unsafe { factory.CreateSpellChecker(&tag) }.map_err(|_| Error::Unavailable)
}

#[derive(Debug)]
pub struct Checker {
    checker: ISpellChecker,
}

impl Checker {
    pub fn new() -> Result<Self, Error> {
        let mut buf = [0u16; 85];
        let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
        if n > 1 {
            if let Ok(tag) = String::from_utf16(&buf[..n as usize - 1]) {
                if let Ok(checker) = open_for_language(&tag) {
                    return Ok(Checker { checker });
                }
            }
        }
        Self::with_locale("en_US", "en-US")
    }

    pub fn with_locale(_hunspell: &str, bcp47: &str) -> Result<Self, Error> {
        Ok(Checker {
            checker: open_for_language(bcp47)?,
        })
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        const MAX: usize = 10;

        if word.is_empty() {
            return Vec::new();
        }

        let Ok(enum_str) = (unsafe { self.checker.Suggest(&HSTRING::from(word)) }) else {
            return Vec::new();
        };

        let mut out = Vec::new();
        while out.len() < MAX {
            let mut item = [PWSTR::null()];
            let mut fetched = 0u32;
            let hr = unsafe { enum_str.Next(&mut item, Some(&mut fetched)) };
            if fetched == 0 || item[0].is_null() {
                break;
            }
            if let Ok(s) = unsafe { item[0].to_string() } {
                out.push(s);
            }
            unsafe {
                CoTaskMemFree(Some(item[0].as_ptr() as *const _));
            }
            if hr.is_err() && hr != S_FALSE {
                break;
            }
        }
        out
    }

    pub fn check(&self, text: &str) -> impl Iterator<Item = SpellingError> + use<> {
        if text.is_empty() {
            return ErrorIter {
                original: String::new(),
                text: vec![],
                iter: None,
            };
        }

        let original = text.to_owned();
        let wide = wide_string(text);
        let iter = unsafe { self.checker.ComprehensiveCheck(&HSTRING::from(text)) }.ok();

        ErrorIter {
            original,
            text: wide,
            iter,
        }
    }

    pub fn ignore(&mut self, word: &str) {
        if word.is_empty() {
            return;
        }
        let _ = unsafe { self.checker.Ignore(&HSTRING::from(word)) };
    }
}

struct ErrorIter {
    original: String,
    text: Vec<u16>,
    iter: Option<IEnumSpellingError>,
}

impl Iterator for ErrorIter {
    type Item = SpellingError;

    fn next(&mut self) -> Option<SpellingError> {
        let iter = self.iter.as_ref()?;

        let mut err: Option<ISpellingError> = None;
        let hr = unsafe { iter.Next(&mut err) };
        if hr == S_FALSE {
            return None;
        }
        let err = err?;

        let start = unsafe { err.StartIndex() }.ok()? as usize;
        let length = unsafe { err.Length() }.ok()? as usize;

        let err_text = String::from_utf16(&self.text[start..start + length]).ok()?;
        let byte_start = utf16_offset_to_utf8(&self.original, start);
        let byte_end = utf16_offset_to_utf8(&self.original, start + length);

        Some(SpellingError {
            text: err_text,
            start: byte_start,
            end: byte_end,
        })
    }
}

pub struct SpellingError {
    text: String,
    start: usize,
    end: usize,
}

impl SpellingError {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn start(&self) -> usize {
        self.start
    }
    pub fn end(&self) -> usize {
        self.end
    }
}
