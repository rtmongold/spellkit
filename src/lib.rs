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

fn normalize_locale(locale: &str) -> (String, String) {
    let mut parts = locale
        .trim()
        .split(['_', '-'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    if let Some(lang) = parts.first_mut() {
        *lang = lang.to_lowercase();
    }
    if let Some(region) = parts.get_mut(1) {
        *region = region.to_uppercase();
    }

    let hunspell = parts.join("_");
    let bcp47 = parts.join("-");
    (hunspell, bcp47)
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
    /// - **Linux:** `LC_ALL` / `LC_MESSAGES` / `LANG` if a Hunspell dictionary exists,
    ///   otherwise `en_US` / `en_GB`.
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
        if locale.trim().is_empty() {
            return Err(Error::InvalidLocale);
        }
        let (hunspell, bcp47) = normalize_locale(locale);
        if hunspell.is_empty() {
            return Err(Error::InvalidLocale);
        }
        Ok(Checker(
            imp::Checker::with_locale(&hunspell, &bcp47)?,
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
    /// Ranges are UTF-8 byte offsets. Linux tokenizes words itself (alphanumeric
    /// and `'`). macOS and Windows use the OS spell-checking APIs, so word breaks
    /// may differ.
    pub fn check<'a>(&self, text: &'a str) -> impl Iterator<Item = SpellingError> + 'a + use<'a> {
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
    use super::{Checker, Error};

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
}
