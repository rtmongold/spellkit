//! `spellkit` is a small crate that binds to the native platform's spell checking APIs and
//! provides a friendlier API.
//!
//! This corresponds to [`ISpellChecker`] on Windows, [`NSSpellChecker`] on MacOS, and [`hunspell`]
//! on other *nix platforms.
//!
//! Spellkit does not bundle dictionaries or implement its own spelling algorithm.
//! It wraps the platform backend and uses system / installed dictionaries.
//! Behavior can differ across operating systems where the underlying APIs differ.
//!
//! # Example
//!
//! ```
//! use spellkit::Checker;
//!
//! let mut checker = Checker::new().unwrap();
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

#[derive(Debug)]
pub enum Error {
    /// Recoverable spell-checker setup failure.
    ///
    /// A single variant on purpose: Linux, macOS, and Windows cannot report the same
    /// failure details. Missing Hunspell files, an unsupported Windows language tag,
    /// and an empty macOS locale all become [`Error::Unavailable`].
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unavailable => write!(f, "spell checker unavailable"),
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
    /// Unknown or unsupported locales behave differently by platform:
    ///
    /// - **Linux:** missing dictionary → [`Error::Unavailable`]
    /// - **macOS:** empty locale → [`Error::Unavailable`]; unknown tags may still
    ///   succeed (system fallback)
    /// - **Windows:** unsupported language tag → [`Error::Unavailable`]
    pub fn with_locale(locale: &str) -> Result<Self, Error> {
        let (hunspell, bcp47) = normalize_locale(locale);
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
}

#[cfg(test)]
mod tests {
    use super::Checker;

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
    #[cfg(all(unix, not(target_os = "macos")))]
    fn with_locale_unknown() {
        assert!(Checker::with_locale("zz_ZZ").is_err());
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
