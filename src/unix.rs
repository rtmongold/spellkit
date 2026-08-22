use crate::Error;

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use hunspell_sys::{
    Hunhandle, Hunspell_add, Hunspell_create, Hunspell_destroy, Hunspell_free_list, Hunspell_spell,
    Hunspell_suggest,
};

const DICT_DIRS: &[&str] = &[
    "/usr/share/hunspell",
    "/usr/share/myspell/dicts",
    "/usr/share/myspell",
    "/usr/local/share/hunspell",
];

const DEFAULT_LOCALES: &[&str] = &["en_US", "en_GB"];

fn find_dictionary(locales: &[&str]) -> Option<(PathBuf, PathBuf)> {
    for dir in DICT_DIRS {
        for locale in locales {
            let aff = Path::new(dir).join(format!("{locale}.aff"));
            let dic = Path::new(dir).join(format!("{locale}.dic"));
            if aff.is_file() && dic.is_file() {
                return Some((aff, dic));
            }
        }
    }
    None
}

fn open_dictionary(locales: &[&str]) -> Result<*mut Hunhandle, Error> {
    let (aff, dic) = find_dictionary(locales).ok_or(Error::Unavailable)?;
    let aff_c = CString::new(aff.as_os_str().as_bytes()).map_err(|_| Error::Unavailable)?;
    let dic_c = CString::new(dic.as_os_str().as_bytes()).map_err(|_| Error::Unavailable)?;
    let hunspell = unsafe { Hunspell_create(aff_c.as_ptr(), dic_c.as_ptr()) };
    if hunspell.is_null() {
        return Err(Error::Unavailable);
    }
    Ok(hunspell)
}

#[derive(Debug)]
pub struct Checker {
    hunspell: *mut Hunhandle,
}

impl Checker {
    pub fn new() -> Result<Self, Error> {
        Ok(Checker {
            hunspell: open_dictionary(DEFAULT_LOCALES)?,
        })
    }

    pub fn with_locale(hunspell_locale: &str, _bcp47: &str) -> Result<Self, Error> {
        Ok(Checker {
            hunspell: open_dictionary(&[hunspell_locale])?,
        })
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        const MAX: usize = 10;

        let Ok(cstr) = CString::new(word) else {
            return Vec::new();
        };

        unsafe {
            let mut list: *mut *mut i8 = ptr::null_mut();
            let n = Hunspell_suggest(
                self.hunspell,
                &mut list,
                cstr.as_bytes_with_nul().as_ptr() as *const i8,
            );
            if n <= 0 || list.is_null() {
                return Vec::new();
            }

            let mut out = Vec::new();
            let take = (n as usize).min(MAX);
            for i in 0..take {
                let p = *list.add(i);
                if p.is_null() {
                    continue;
                }
                if let Ok(s) = CStr::from_ptr(p).to_str() {
                    out.push(s.to_owned());
                }
            }
            Hunspell_free_list(self.hunspell, &mut list, n);
            out
        }
    }

    pub fn check<'a>(&self, text: &'a str) -> impl Iterator<Item = SpellingError> + 'a + use<'a> {
        let hunspell = self.hunspell;

        words(text).filter_map(move |(start, end, word)| {
            let cstr = CString::new(word).ok()?;
            let ok =
                unsafe { Hunspell_spell(hunspell, cstr.as_bytes_with_nul().as_ptr() as *const i8) }
                    != 0;
            if ok {
                None
            } else {
                Some(SpellingError {
                    text: word.to_owned(),
                    start,
                    end,
                })
            }
        })
    }

    pub fn ignore(&mut self, word: &str) {
        let Ok(cstr) = CString::new(word) else {
            return;
        };

        unsafe {
            Hunspell_add(
                self.hunspell,
                cstr.as_bytes_with_nul().as_ptr() as *const i8,
            )
        };
    }
}

impl Drop for Checker {
    fn drop(&mut self) {
        unsafe {
            Hunspell_destroy(self.hunspell);
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

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

fn words(text: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut words = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((start, c)) = chars.next() {
        if !is_word_char(c) {
            continue;
        }

        let mut end = start + c.len_utf8();
        while let Some(&(i, next)) = chars.peek() {
            if !is_word_char(next) {
                break;
            }
            end = i + next.len_utf8();
            chars.next();
        }
        words.push((start, end, &text[start..end]));
    }
    words.into_iter()
}
