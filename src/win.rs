use crate::Error;

use std::cell::{Cell, RefCell};

use windows::{
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
        Globalization::{
            IEnumSpellingError, ISpellChecker, ISpellCheckerFactory, ISpellingError,
            SpellCheckerFactory,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
    },
    core::{HSTRING, PWSTR},
};

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

thread_local! {
    static COM_OWNED: Cell<bool> = const { Cell::new(false) };
}

fn ensure_com() -> Result<(), Error> {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr == S_OK {
        COM_OWNED.with(|owned| {
            if !owned.get() {
                owned.set(true);
            }
        });
        return Ok(());
    }
    if hr == S_FALSE || hr == RPC_E_CHANGED_MODE {
        return Ok(());
    }
    Err(Error::InitializationFailed {
        locale: None,
        message: format!("CoInitializeEx failed: {hr:?}"),
    })
}

struct ComThreadGuard;

impl Drop for ComThreadGuard {
    fn drop(&mut self) {
        COM_OWNED.with(|owned| {
            if owned.replace(false) {
                unsafe { CoUninitialize() };
            }
        });
    }
}

thread_local! {
    static COM_GUARD: RefCell<Option<ComThreadGuard>> = const { RefCell::new(None) };
}

fn ensure_com_guarded() -> Result<(), Error> {
    ensure_com()?;
    COM_GUARD.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_none() && COM_OWNED.with(|o| o.get()) {
            *g = Some(ComThreadGuard);
        }
    });
    Ok(())
}

fn open_for_language(bcp47: &str) -> Result<ISpellChecker, Error> {
    if bcp47.is_empty() {
        return Err(Error::InvalidLocale);
    }
    ensure_com_guarded()?;

    let factory: ISpellCheckerFactory =
        unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) }.map_err(
            |e| Error::InitializationFailed {
                locale: Some(bcp47.to_owned()),
                message: e.to_string(),
            },
        )?;

    let tag = HSTRING::from(bcp47);
    let supported =
        unsafe { factory.IsSupported(&tag) }.map_err(|e| Error::InitializationFailed {
            locale: Some(bcp47.to_owned()),
            message: e.to_string(),
        })?;
    if !supported.as_bool() {
        return Err(Error::UnsupportedLocale {
            locale: bcp47.to_owned(),
        });
    }
    unsafe { factory.CreateSpellChecker(&tag) }.map_err(|e| Error::InitializationFailed {
        locale: Some(bcp47.to_owned()),
        message: e.to_string(),
    })
}

fn supported_language_tags() -> Vec<String> {
    if ensure_com_guarded().is_err() {
        return Vec::new();
    }
    let factory: ISpellCheckerFactory =
        match unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) } {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
    let Ok(enum_str) = (unsafe { factory.SupportedLanguages() }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
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
    out.sort();
    out.dedup();
    out
}

#[derive(Debug)]
pub struct Checker {
    checker: ISpellChecker,
    locale: String,
}

impl Checker {
    pub fn new() -> Result<Self, Error> {
        let mut buf = [0u16; 85];
        let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) };
        if n > 1 {
            if let Ok(tag) = String::from_utf16(&buf[..n as usize - 1]) {
                if let Ok(checker) = open_for_language(&tag) {
                    return Ok(Checker {
                        checker,
                        locale: tag,
                    });
                }
            }
        }
        Self::with_locale("en_US", "en_US", "en-US")
    }

    pub fn with_locale(_raw: &str, _hunspell: &str, bcp47: &str) -> Result<Self, Error> {
        Ok(Checker {
            checker: open_for_language(bcp47)?,
            locale: bcp47.to_owned(),
        })
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn available_locales() -> Vec<String> {
        supported_language_tags()
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
                iter: None,
            };
        }

        let original = text.to_owned();
        let iter = unsafe { self.checker.ComprehensiveCheck(&HSTRING::from(text)) }.ok();

        ErrorIter { original, iter }
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
    iter: Option<IEnumSpellingError>,
}

impl Iterator for ErrorIter {
    type Item = SpellingError;

    fn next(&mut self) -> Option<SpellingError> {
        let iter = self.iter.as_ref()?;

        loop {
            let mut err: Option<ISpellingError> = None;
            let hr = unsafe { iter.Next(&mut err) };
            if hr == S_FALSE {
                return None;
            }
            let Some(err) = err else {
                return None;
            };

            let Ok(start) = (unsafe { err.StartIndex() }) else {
                continue;
            };
            let Ok(length) = (unsafe { err.Length() }) else {
                continue;
            };
            let start = start as usize;
            let length = length as usize;
            let Some(end_units) = start.checked_add(length) else {
                continue;
            };

            let byte_start = utf16_offset_to_utf8(&self.original, start);
            let byte_end = utf16_offset_to_utf8(&self.original, end_units);
            if !self.original.is_char_boundary(byte_start)
                || !self.original.is_char_boundary(byte_end)
                || byte_start > byte_end
            {
                continue;
            }
            let Some(err_text) = self.original.get(byte_start..byte_end) else {
                continue;
            };
            if err_text.is_empty() {
                continue;
            }

            return Some(SpellingError {
                text: err_text.to_owned(),
                start: byte_start,
                end: byte_end,
            });
        }
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
