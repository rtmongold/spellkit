use crate::Error;

use std::ptr;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{NSInteger, NSNotFound, NSRange, NSString};

/// `NSSpellChecker` is not thread-safe; serialize access.
fn with_checker<R>(f: impl FnOnce(&NSSpellChecker) -> R) -> R {
    static LOCK: Mutex<()> = Mutex::new(());
    let _lock = LOCK.lock().unwrap();
    f(&NSSpellChecker::sharedSpellChecker())
}

fn ns_string(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn language_ref(language: &str) -> Retained<NSString> {
    ns_string(language)
}

fn mac_locale_supported(requested_bcp47: &str, available: &str) -> bool {
    let want = requested_bcp47.replace('_', "-");
    let have = available.replace('_', "-");
    if want.eq_ignore_ascii_case(&have) {
        return true;
    }
    let want_lang = want.split('-').next().unwrap_or("");
    let have_parts: Vec<_> = have.split('-').collect();
    have_parts.len() == 1 && have_parts[0].eq_ignore_ascii_case(want_lang)
}

fn nsarray_to_strings(
    array: Option<&objc2_foundation::NSArray<NSString>>,
    max: usize,
) -> Vec<String> {
    let Some(array) = array else {
        return Vec::new();
    };
    let take = (array.count() as usize).min(max);
    let mut out = Vec::with_capacity(take);
    for i in 0..take {
        out.push(array.objectAtIndex(i as _).to_string());
    }
    out
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

#[derive(Debug)]
pub struct Checker {
    document_tag: NSInteger,
    language: String,
}

impl Drop for Checker {
    fn drop(&mut self) {
        let tag = self.document_tag;
        with_checker(|c| c.closeSpellDocumentWithTag(tag));
    }
}

impl Checker {
    pub fn new() -> Result<Self, Error> {
        let language = with_checker(|c| c.language().to_string());
        if language.is_empty() {
            return Err(Error::InitializationFailed {
                locale: None,
                message: "NSSpellChecker.language is empty".into(),
            });
        }
        Ok(Self {
            document_tag: NSSpellChecker::uniqueSpellDocumentTag(),
            language,
        })
    }

    pub fn with_locale(_hunspell: &str, bcp47: &str) -> Result<Self, Error> {
        if bcp47.is_empty() {
            return Err(Error::InvalidLocale);
        }
        let available =
            with_checker(|c| nsarray_to_strings(Some(c.availableLanguages().as_ref()), usize::MAX));
        let ok = available.iter().any(|t| mac_locale_supported(bcp47, t));
        if !ok {
            return Err(Error::UnsupportedLocale {
                locale: bcp47.to_owned(),
            });
        }
        Ok(Self {
            document_tag: NSSpellChecker::uniqueSpellDocumentTag(),
            language: bcp47.to_owned(),
        })
    }

    pub fn locale(&self) -> &str {
        &self.language
    }

    pub fn available_locales() -> Vec<String> {
        with_checker(|c| nsarray_to_strings(Some(c.availableLanguages().as_ref()), usize::MAX))
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        const MAX: usize = 10;
        if word.is_empty() {
            return Vec::new();
        }

        let ns_word = ns_string(word);
        let lang = language_ref(&self.language);
        let range = NSRange {
            location: 0,
            length: word.encode_utf16().count(),
        };
        let tag = self.document_tag;

        let guesses = with_checker(|c| {
            c.guessesForWordRange_inString_language_inSpellDocumentWithTag(
                range,
                &ns_word,
                Some(&*lang),
                tag,
            )
        });
        nsarray_to_strings(guesses.as_deref(), MAX)
    }

    pub fn ignore(&mut self, word: &str) {
        let ns_word = ns_string(word);
        let tag = self.document_tag;
        with_checker(|c| c.ignoreWord_inSpellDocumentWithTag(&ns_word, tag));
    }

    pub fn check(&self, text: &str) -> impl Iterator<Item = SpellingError> + use<> {
        SpellcheckIter {
            document_tag: self.document_tag,
            ns_text: ns_string(text),
            ns_offset: 0,
            original: text.to_owned(),
            language: self.language.clone(),
        }
    }
}

#[derive(Debug)]
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

struct SpellcheckIter {
    document_tag: NSInteger,
    ns_text: Retained<NSString>,
    ns_offset: usize,
    original: String,
    language: String,
}

impl Iterator for SpellcheckIter {
    type Item = SpellingError;

    fn next(&mut self) -> Option<Self::Item> {
        let lang = language_ref(&self.language);
        let tag = self.document_tag;
        let ns_text = self.ns_text.clone();
        let starting = self.ns_offset as NSInteger;

        let range = with_checker(|c| unsafe {
            c.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                &ns_text,
                starting,
                Some(&*lang),
                false,
                tag,
                ptr::null_mut(),
            )
        });

        if range.length == 0 || range.location == NSNotFound as usize {
            return None;
        }

        let utf16_start = range.location;
        let utf16_end = range.location + range.length;
        let start = utf16_offset_to_utf8(&self.original, utf16_start);
        let end = utf16_offset_to_utf8(&self.original, utf16_end);
        let misspelling = self.original[start..end].to_owned();

        self.ns_offset = utf16_end;

        Some(SpellingError {
            text: misspelling,
            start,
            end,
        })
    }
}
