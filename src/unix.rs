use crate::Error;

use std::collections::HashSet;
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

const DEFAULT_LOCALES: &[&str] = &["en_US", "en_GB", "en"];

fn dict_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dicpath) = std::env::var("DICPATH") {
        for part in dicpath.split(':') {
            if !part.is_empty() {
                dirs.push(PathBuf::from(part));
            }
        }
    }
    for dir in DICT_DIRS {
        dirs.push(PathBuf::from(dir));
    }
    dirs
}

fn find_dictionary(dirs: &[PathBuf], locales: &[&str]) -> Option<(PathBuf, PathBuf, String)> {
    for dir in dirs {
        for locale in locales {
            let aff = Path::new(dir).join(format!("{locale}.aff"));
            let dic = Path::new(dir).join(format!("{locale}.dic"));
            if aff.is_file() && dic.is_file() {
                return Some((aff, dic, (*locale).to_owned()));
            }
        }
    }
    None
}

#[derive(Clone, Debug)]
struct WordChars {
    /// Extra chars beyond alphanumeric and ASCII `'`.
    extra: HashSet<char>,
}

impl WordChars {
    fn contains(&self, c: char) -> bool {
        c.is_alphanumeric() || c == '\'' || self.extra.contains(&c)
    }

    fn from_aff(aff: &Path) -> Self {
        let mut extra = HashSet::new();
        let Ok(contents) = std::fs::read_to_string(aff) else {
            return Self { extra };
        };

        let mut lines = contents.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#") {
                continue;
            }

            if let Some(rest) = line.strip_prefix("WORDCHARS") {
                for c in rest.trim().chars() {
                    if !c.is_whitespace() {
                        extra.insert(c);
                    }
                }
                continue;
            }

            // `ICONV N` then N mapping lines: `from to` or `ICONV from to`
            let mut parts = line.split_whitespace();
            if parts.next() != Some("ICONV") {
                continue;
            }
            let Some(count_str) = parts.next() else {
                continue;
            };
            let Ok(count) = count_str.parse::<usize>() else {
                continue;
            };
            if parts.next().is_some() {
                // Same line is a mapping (`ICONV ' '`), not a count.
                continue;
            }

            for _ in 0..count {
                let Some(map_line) = lines.next() else {
                    break;
                };
                let map_line = map_line.trim();
                let map_line = map_line.strip_prefix("ICONV").unwrap_or(map_line).trim();
                let mut map = map_line.split_whitespace();
                let Some(from) = map.next() else {
                    continue;
                };
                let Some(to) = map.next() else {
                    continue;
                };
                // Add ICONV sources whose dest is `'` or already a word char.
                let to_is_word = to
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '\'' || extra.contains(&c));
                if to == "'" || to_is_word {
                    for c in from.chars() {
                        extra.insert(c);
                    }
                }
            }
        }

        Self { extra }
    }
}

fn open_dictionary(locales: &[&str]) -> Result<(*mut Hunhandle, String, WordChars), Error> {
    let dirs = dict_search_dirs();
    let Some((aff, dic, locale)) = find_dictionary(&dirs, locales) else {
        return Err(Error::DictionaryNotFound {
            locale: locales.join(","),
            searched: dirs,
        });
    };
    let word_chars = WordChars::from_aff(&aff);
    let label = Some(locale.clone());
    let aff_c =
        CString::new(aff.as_os_str().as_bytes()).map_err(|_| Error::InitializationFailed {
            locale: label.clone(),
            message: "dictionary path contains NUL".into(),
        })?;
    let dic_c =
        CString::new(dic.as_os_str().as_bytes()).map_err(|_| Error::InitializationFailed {
            locale: label.clone(),
            message: "dictionary path contains NUL".into(),
        })?;
    let hunspell = unsafe { Hunspell_create(aff_c.as_ptr(), dic_c.as_ptr()) };
    if hunspell.is_null() {
        return Err(Error::InitializationFailed {
            locale: label.clone(),
            message: "Hunspell_create returned null".into(),
        });
    }
    Ok((hunspell, locale, word_chars))
}

#[derive(Debug)]
pub struct Checker {
    hunspell: *mut Hunhandle,
    locale: String,
    word_chars: WordChars,
}

impl Checker {
    pub fn new() -> Result<Self, Error> {
        if let Some(raw) = crate::locale_from_vars(|key| std::env::var(key).ok()) {
            let (hunspell_tag, _) = crate::normalize_locale(&raw);
            let mut candidates = crate::lookup_candidates(&raw, &hunspell_tag);
            if let Some((lang, _)) = hunspell_tag.split_once('_') {
                if !candidates.contains(&lang) {
                    candidates.push(lang);
                }
            }
            if let Ok((handle, locale, word_chars)) = open_dictionary(&candidates) {
                return Ok(Checker {
                    hunspell: handle,
                    locale,
                    word_chars,
                });
            }
        }
        let (handle, locale, word_chars) = open_dictionary(DEFAULT_LOCALES)?;
        Ok(Checker {
            hunspell: handle,
            locale,
            word_chars,
        })
    }

    pub fn with_locale(raw: &str, hunspell_locale: &str, _bcp47: &str) -> Result<Self, Error> {
        let candidates = crate::lookup_candidates(raw, hunspell_locale);
        let (hunspell, locale, word_chars) = open_dictionary(&candidates)?;
        Ok(Checker {
            hunspell,
            locale,
            word_chars,
        })
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn available_locales() -> Vec<String> {
        let mut out = Vec::new();
        for dir in dict_search_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for ent in entries.flatten() {
                let path = ent.path();
                if path.extension().and_then(|e| e.to_str()) != Some("dic") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_owned());
                }
            }
        }
        out.sort();
        out.dedup();
        out
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

    pub fn check<'a>(&'a self, text: &'a str) -> impl Iterator<Item = SpellingError> + 'a {
        let hunspell = self.hunspell;
        let word_chars = &self.word_chars;
        words(text, word_chars).filter_map(move |(start, end, word)| {
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

fn trim_word_edges<'a>(
    text: &'a str,
    start: usize,
    end: usize,
    wc: &WordChars,
) -> Option<(usize, usize, &'a str)> {
    let mut s = start;
    let mut e = end;
    while s < e {
        let c = text[s..e].chars().next()?;
        if c.is_alphanumeric() {
            break;
        }
        if !wc.contains(c) {
            break;
        }
        s += c.len_utf8();
    }
    while e > s {
        let c = text[s..e].chars().next_back()?;
        if c.is_alphanumeric() {
            break;
        }
        if !wc.contains(c) {
            break;
        }
        e -= c.len_utf8();
    }
    if s >= e {
        return None;
    }
    Some((s, e, &text[s..e]))
}

/// If `text[from..]` starts a URL or email token, return the byte index after it.
fn skip_url_or_email(text: &str, from: usize) -> Option<usize> {
    let rest = &text[from..];
    let token_end = rest
        .find(|c: char| c.is_whitespace())
        .map(|o| from + o)
        .unwrap_or(text.len());
    let token = &text[from..token_end];
    if token.contains("://") {
        return Some(token_end);
    }
    if let Some(at) = token.find('@') {
        let domain = &token[at + 1..];
        if domain.contains('.') {
            return Some(token_end);
        }
    }
    None
}

struct Words<'a> {
    text: &'a str,
    word_chars: &'a WordChars,
    pos: usize,
}

impl<'a> Iterator for Words<'a> {
    type Item = (usize, usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.text.as_bytes();
        while self.pos < bytes.len() {
            // Skip whitespace.
            let c = self.text[self.pos..].chars().next()?;
            if c.is_whitespace() {
                self.pos += c.len_utf8();
                continue;
            }

            if let Some(after) = skip_url_or_email(self.text, self.pos) {
                self.pos = after;
                continue;
            }

            if !self.word_chars.contains(c) {
                self.pos += c.len_utf8();
                continue;
            }

            let start = self.pos;
            self.pos += c.len_utf8();
            while self.pos < bytes.len() {
                let next = self.text[self.pos..].chars().next()?;
                if !self.word_chars.contains(next) {
                    break;
                }
                self.pos += next.len_utf8();
            }
            let end = self.pos;

            if let Some(item) = trim_word_edges(self.text, start, end, self.word_chars) {
                return Some(item);
            }
        }
        None
    }
}

fn words<'a>(text: &'a str, word_chars: &'a WordChars) -> Words<'a> {
    Words {
        text,
        word_chars,
        pos: 0,
    }
}

#[cfg(test)]
mod word_tests {
    use super::*;
    #[test]
    fn trim_keeps_interior_apostrophe() {
        let wc = WordChars {
            extra: HashSet::new(),
        };
        let text = "'don't'";
        let (s, e, w) = trim_word_edges(text, 0, text.len(), &wc).unwrap();
        assert_eq!(w, "don't");
        assert_eq!(&text[s..e], "don't");
    }
}
