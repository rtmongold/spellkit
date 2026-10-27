//! Cross-platform native spell checking: [`NSSpellChecker`] on macOS, [`ISpellChecker`]
//! on Windows, and [`hunspell`] with system dictionaries on other Unix.
//!
//! Spellkit does not bundle dictionaries or implement its own spelling algorithm.
//! Use it when you want OS dictionaries and a small API; use a crate like Spellbook
//! when you want a portable engine and app-shipped word lists.
//! Behavior can differ across operating systems where the backends differ.
//!
//! # Example
//!
//! ```
//! use spellkit::Checker;
//!
//! let checker = Checker::new().unwrap();
//!
//! let errors: Vec<_> = checker.check("I beleeve I can fly").collect();
//!
//! assert_eq!(errors.len(), 1);
//! assert_eq!(errors[0].text(), "beleeve");
//! ```
//!
//! ```compile_fail
//! fn needs_send<T: Send>() {}
//! needs_send::<spellkit::Checker>();
//! ```
//!
//! [`ISpellChecker`]: https://docs.microsoft.com/en-us/windows/desktop/api/spellcheck/nn-spellcheck-ispellchecker
//! [`NSSpellChecker`]: https://developer.apple.com/documentation/appkit/nsspellchecker
//! [`hunspell`]: https://hunspell.github.io/
use cfg_if::cfg_if;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Range;
use std::path::PathBuf;

/// Failure creating a [`Checker`].
///
/// Match on this instead of a single “unavailable” flag: Linux can report missing
/// Hunspell files, while macOS and Windows report an unsupported language tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The locale string was empty or could not be normalized.
    InvalidLocale,
    /// The OS has no spell checker for this language (macOS / Windows).
    UnsupportedLocale { locale: String },
    /// No Hunspell `.aff` / `.dic` pair was found (Linux / other Unix).
    ///
    /// `searched` is the directory list that was walked (`DICPATH` then the
    /// built-in system paths).
    DictionaryNotFound {
        locale: String,
        searched: Vec<PathBuf>,
    },
    /// The backend started but failed (null Hunspell handle, COM factory, empty
    /// macOS system language).
    InitializationFailed {
        locale: Option<String>,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidLocale => write!(f, "invalid locale"),
            Error::UnsupportedLocale { locale } => write!(f, "unsupported locale: {locale}"),
            Error::DictionaryNotFound { locale, searched } => write!(
                f,
                "dictionary not found for locale: {locale}, searched: {searched:?}"
            ),
            Error::InitializationFailed { locale, message } => write!(
                f,
                "initialization failed for locale: {locale:?}, message: {message}"
            ),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn strip_posix_locale(locale: &str) -> &str {
    locale.trim().split(['.', '@']).next().unwrap_or("").trim()
}

fn case_subtag(index: usize, part: &str) -> String {
    if index == 0 {
        return part.to_ascii_lowercase();
    }
    let alpha = part.chars().all(|c| c.is_ascii_alphabetic());
    let digit = part.chars().all(|c| c.is_ascii_digit());
    if alpha && part.len() == 4 {
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            return String::new();
        };
        let mut out = String::new();
        out.push(first.to_ascii_uppercase());
        out.extend(chars.map(|c| c.to_ascii_lowercase()));
        return out;
    }
    if (alpha && part.len() == 2) || (digit && part.len() == 3) {
        return part.to_ascii_uppercase();
    }
    part.to_owned()
}

pub(crate) fn normalize_locale(locale: &str) -> (String, String) {
    let parts: Vec<String> = strip_posix_locale(locale)
        .split(['_', '-'])
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, part)| case_subtag(i, part))
        .collect();
    (parts.join("_"), parts.join("-"))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn lookup_candidates<'a>(raw: &'a str, hunspell: &'a str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut push = |s: &'a str| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    push(raw);
    push(hunspell);
    if hunspell == "en" {
        push("en_US");
        push("en_GB");
    }
    out
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
pub(crate) fn locale_from_vars(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        let Some(val) = get(key) else {
            continue;
        };
        let base = strip_posix_locale(&val);
        if base.is_empty() || base == "C" || base == "POSIX" {
            continue;
        }
        return Some(base.to_owned());
    }
    None
}

cfg_if! {
    if #[cfg(target_os = "macos")] {
        mod mac;
        use crate::mac as imp;
    } else if #[cfg(windows)] {
        mod win;
        use crate::win as imp;
    } else if #[cfg(unix)] {
        mod unix;
        use crate::unix as imp;
    } else {
        compile_error!("target platform is not supported");
    }
}

/// Instance of the system spell checker.
///
/// `Checker` is not `Send` or `Sync`. Do not share it across threads.
/// macOS also serializes access to the shared `NSSpellChecker`.
#[derive(Debug)]
pub struct Checker(imp::Checker, PhantomData<*const ()>);

impl Checker {
    /// Create a checker with a platform default locale.
    ///
    /// - **Linux:** `LC_ALL` / `LC_CTYPE` / `LANG` if a Hunspell dictionary exists,
    ///   otherwise `en_US` / `en_GB` / `en`.
    /// - **macOS:** the system default language
    /// - **Windows:** the user locale if the OS has a checker, otherwise `en-US`
    pub fn new() -> Result<Self, Error> {
        Ok(Checker(imp::Checker::new()?, PhantomData))
    }

    /// Create a checker for a specific locale (`en_US` or `en-US` both work).
    ///
    /// - empty string - [`Error::InvalidLocale`]
    /// - **Linux:** missing `.aff` / `.dic` → [`Error::DictionaryNotFound`]
    /// - **macOS / Windows:** language not installed → [`Error::UnsupportedLocale`]
    pub fn with_locale(locale: &str) -> Result<Self, Error> {
        let raw = strip_posix_locale(locale);
        if raw.is_empty() {
            return Err(Error::InvalidLocale);
        }
        let (hunspell, bcp47) = normalize_locale(raw);
        if hunspell.is_empty() {
            return Err(Error::InvalidLocale);
        }
        Ok(Checker(
            imp::Checker::with_locale(raw, &hunspell, &bcp47)?,
            PhantomData,
        ))
    }

    /// Spelling suggestions for `word`.
    ///
    /// Returns at most 10 suggestions, or an empty list if none are available.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        self.0.suggest(word)
    }

    /// Check `text` for spelling errors.
    ///
    /// Ranges are UTF-8 byte offsets. Linux tokenizes with Hunspell `WORDCHARS` /
    /// `ICONV` from the dictionary `.aff`, skips URLs and emails, and trims
    /// leading/trailing quote-like word characters. macOS and Windows use the OS APIs.
    pub fn check<'a>(
        &'a self,
        text: &'a str,
    ) -> impl Iterator<Item = SpellingError> + 'a + use<'a> {
        self.0.check(text).map(SpellingError)
    }

    /// Returns true if `word` is spelled correctly.
    pub fn is_correct(&self, word: &str) -> bool {
        self.check(word).next().is_none()
    }

    /// Instructs the spell checker to ignore a word in future checks. The word is temporarily
    /// added to the spell checker's ignore list, and other instances of the spell checker will not
    /// ignore the word.
    pub fn ignore(&mut self, word: &str) {
        self.0.ignore(word)
    }

    /// Language tag this checker is using (Hunspell `en_US` or BCP-47 `en-US`).
    ///
    /// After [`Checker::new`] this is the locale that was actually selected, not
    /// a placeholder for “system default.”
    pub fn locale(&self) -> &str {
        self.0.locale()
    }

    /// Locales the OS can check.
    ///
    /// Linux: Hunspell `*.dic` stems on `DICPATH` and the system dict dirs.
    /// macOS: `NSSpellChecker` available languages. Windows: `SupportedLanguages`.
    pub fn available_locales() -> Vec<String> {
        imp::Checker::available_locales()
    }
}

/// A spelling error.
pub struct SpellingError(imp::SpellingError);

impl SpellingError {
    /// Returns the text of the misspelled word.
    pub fn text(&self) -> &str {
        self.0.text()
    }
    /// Inclusive start index of the misspelling, as a UTF-8 byte offset into the
    /// original text. `&text[start()..end()]` is the misspelled word.
    pub fn start(&self) -> usize {
        self.0.start()
    }
    /// Exclusive end index of the misspelling, as a UTF-8 byte offset into the
    /// original text. `&text[start()..end()]` is the misspelled word.
    pub fn end(&self) -> usize {
        self.0.end()
    }

    /// UTF-8 byte range of the misspelling: `start()..end()`.
    pub fn range(&self) -> Range<usize> {
        self.start()..self.end()
    }
}

impl fmt::Display for SpellingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @ {}..{}", self.text(), self.start(), self.end())
    }
}

#[cfg(test)]
mod tests {
    use super::{Checker, Error, locale_from_vars, normalize_locale};

    #[test]
    fn no_errors() {
        let text = "I'm happy that this sentence has no errors.";
        let checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(checker.check(text).count(), 0);
    }

    #[test]
    fn single_error() {
        let text = "beleeve";
        let checker = Checker::with_locale("en_US").unwrap();
        let errors = checker.check(text).collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].text(), "beleeve");
        assert_eq!(&text[errors[0].start()..errors[0].end()], "beleeve");
    }

    #[test]
    fn multiple_errors() {
        let text = "asdf hjkl qwer uiop";
        let checker = Checker::with_locale("en_US").unwrap();
        let errors = checker.check(text).collect::<Vec<_>>();
        assert_eq!(errors.len(), 4);
        assert_eq!(errors[0].text(), "asdf");
        assert_eq!(errors[1].text(), "hjkl");
        assert_eq!(errors[2].text(), "qwer");
        assert_eq!(errors[3].text(), "uiop");
    }

    #[test]
    fn error_ranges() {
        let text = "one asdf two";
        let checker = Checker::with_locale("en_US").unwrap();
        let errors: Vec<_> = checker.check(text).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].text(), "asdf");
        assert_eq!(errors[0].start(), 4);
        assert_eq!(errors[0].end(), 8);
        assert_eq!(&text[errors[0].start()..errors[0].end()], "asdf");
    }

    #[test]
    fn empty() {
        let checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(checker.check("").count(), 0);
    }

    #[test]
    fn ignore() {
        let mut checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(checker.check("foobarbaz").count(), 1);
        checker.ignore("foobarbaz");
        assert_eq!(checker.check("foobarbaz").count(), 0);
    }

    #[test]
    fn ignore_not_permanent() {
        let mut checker = Checker::with_locale("en_US").unwrap();
        checker.ignore("foobarbaz");
        drop(checker);
        let checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(checker.check("foobarbaz").count(), 1);
    }

    #[test]
    fn with_locale_en_us() {
        assert!(Checker::with_locale("en_US").is_ok());
        assert!(Checker::with_locale("en-US").is_ok());
    }

    #[test]
    fn with_locale_empty() {
        assert!(matches!(
            Checker::with_locale(""),
            Err(Error::InvalidLocale)
        ));
    }

    #[test]
    fn normalize_locale_tags() {
        assert_eq!(
            normalize_locale("en_US.UTF-8"),
            ("en_US".into(), "en-US".into())
        );
        assert_eq!(
            normalize_locale("en_US.UTF-8@euro"),
            ("en_US".into(), "en-US".into())
        );
        assert_eq!(normalize_locale("en-us"), ("en_US".into(), "en-US".into()));
        assert_eq!(
            normalize_locale("zh-Hans-CN"),
            ("zh_Hans_CN".into(), "zh-Hans-CN".into())
        );
        assert_eq!(
            normalize_locale("zh_hans_cn"),
            ("zh_Hans_CN".into(), "zh-Hans-CN".into())
        );
        assert_eq!(
            normalize_locale("sr-Latn-RS"),
            ("sr_Latn_RS".into(), "sr-Latn-RS".into())
        );
        assert_eq!(
            normalize_locale("es-419"),
            ("es_419".into(), "es-419".into())
        );
        assert_eq!(
            normalize_locale("en_US-large"),
            ("en_US_large".into(), "en-US-large".into())
        );
    }

    #[test]
    fn locale_from_vars_order() {
        let get = |key: &str| match key {
            "LC_MESSAGES" => Some("fr_FR".into()),
            "LC_CTYPE" => Some("de_DE.UTF-8".into()),
            "LANG" => Some("en_US".into()),
            _ => None,
        };
        assert_eq!(locale_from_vars(get).as_deref(), Some("de_DE"));

        let get = |key: &str| match key {
            "LC_ALL" => Some("C".into()),
            "LANG" => Some("en_GB.UTF-8".into()),
            _ => None,
        };
        assert_eq!(locale_from_vars(get).as_deref(), Some("en_GB"));
    }

    #[test]
    fn with_locale_encoding_only() {
        assert!(matches!(
            Checker::with_locale(".UTF-8"),
            Err(Error::InvalidLocale)
        ));
        assert!(matches!(
            Checker::with_locale("@euro"),
            Err(Error::InvalidLocale)
        ));
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn with_locale_raw_stem_and_bare_en() {
        assert!(Checker::with_locale("en_US.UTF-8").is_ok());
        assert!(Checker::with_locale("en-us").is_ok());
        let checker = Checker::with_locale("en").unwrap();
        assert!(matches!(checker.locale(), "en" | "en_US" | "en_GB"));
        if std::path::Path::new("/usr/share/hunspell/en_US-large.aff").is_file()
            && std::path::Path::new("/usr/share/hunspell/en_US-large.dic").is_file()
        {
            let checker = Checker::with_locale("en_US-large").unwrap();
            assert_eq!(checker.locale(), "en_US-large");
        }
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn with_locale_unknown() {
        match Checker::with_locale("zz_ZZ") {
            Err(Error::DictionaryNotFound { locale, searched }) => {
                assert!(locale.contains("zz"));
                assert!(!searched.is_empty());
            }
            other => panic!("expected DictionaryNotFound, got {other:?}"),
        }
    }

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn with_locale_unknown() {
        match Checker::with_locale("zz_ZZ") {
            Err(Error::UnsupportedLocale { locale }) => {
                assert!(locale.to_lowercase().contains("zz"));
            }
            other => panic!("expected UnsupportedLocale, got {other:?}"),
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn unix_locale_or_skip(locales: &[&str]) -> Option<Checker> {
        for tag in locales {
            match Checker::with_locale(tag) {
                Ok(c) => return Some(c),
                Err(Error::DictionaryNotFound { .. }) => continue,
                Err(e) => panic!("{e}"),
            }
        }
        if std::env::var_os("CI").is_some() {
            panic!("missing Hunspell dicts for {locales:?} (CI must install them)");
        }
        None
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn hunspell_de_de() {
        let Some(checker) = unix_locale_or_skip(&["de_DE", "de"]) else {
            return;
        };
        assert!(checker.is_correct("Haus"));
        assert!(!checker.is_correct("Hauzz"));
        assert!(!checker.suggest("Hauzz").is_empty());
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn hunspell_fr() {
        let Some(checker) = unix_locale_or_skip(&["fr_FR", "fr"]) else {
            return;
        };
        assert!(checker.is_correct("bonjour"));
        assert!(!checker.is_correct("bonjoour"));
        assert!(!checker.suggest("bonjoour").is_empty());
    }

    #[test]
    fn utf8_range() {
        let text = "café beleeve";
        let checker = Checker::with_locale("en_US").unwrap();
        let errors: Vec<_> = checker.check(text).collect();
        let e = errors
            .iter()
            .find(|e| e.text() == "beleeve")
            .unwrap_or_else(|| {
                panic!(
                    "expected beleeve, got {:?}",
                    errors.iter().map(|e| e.text()).collect::<Vec<_>>()
                )
            });
        assert_eq!(&text[e.start()..e.end()], "beleeve");
        assert_eq!(e.range(), e.start()..e.end());
        assert!(e.start() > 0);
    }

    #[test]
    fn locale_en() {
        let checker = Checker::with_locale("en_US").unwrap();
        assert!(checker.locale().to_lowercase().contains("en"));
    }

    #[test]
    fn available_locales_nonempty() {
        assert!(!Checker::available_locales().is_empty());
    }

    #[test]
    fn suggest_misspelling() {
        let checker = Checker::with_locale("en_US").unwrap();
        let suggestions = checker.suggest("beleeve");
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn is_correct() {
        let checker = Checker::with_locale("en_US").unwrap();
        assert!(checker.is_correct("believe"));
        assert!(!checker.is_correct("beleeve"));
    }

    #[test]
    fn ignore_interior_nul() {
        let mut checker = Checker::with_locale("en_US").unwrap();
        checker.ignore("foo\0bar");
    }

    #[test]
    fn new_succeeds() {
        assert!(Checker::with_locale("en_US").is_ok());
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn curly_apostrophe_is_one_word() {
        let checker = Checker::with_locale("en_US").unwrap();
        // U+2019 RIGHT SINGLE QUOTATION MARK between n and t
        assert_eq!(checker.check("don\u{2019}t").count(), 0);
        assert_eq!(checker.check("don't").count(), 0);
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn quoted_word_strips_quotes() {
        let checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(checker.check("'hello'").count(), 0);
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn skips_url_and_email() {
        let checker = Checker::with_locale("en_US").unwrap();
        assert_eq!(
            checker
                .check("see https://example.com/foo and a@b.co please")
                .count(),
            0
        );
    }
}
