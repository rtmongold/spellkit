# Changelog

All notable changes to this project are documented in this file.

This project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Linux `Checker::check` iterators now borrow the checker (needed for affix-based word breaks)

### Fixed
- Locale tags keep script and region case (`zh-Hans-CN`), and a POSIX encoding or `@modifier` is stripped before lookup
- Linux opens the dictionary stem it was given (`en_US-large`) before the normalized name
- Linux `Checker::new()` reads `LC_CTYPE` instead of `LC_MESSAGES`, and falls back to a bare `en` dictionary
- `with_locale("en")` tries `en`, then `en_US`, then `en_GB`
- macOS `with_locale` stores the installed language tag that matched, and accepts a bare language when only a regional tag is installed
- Linux tokenization uses Hunspell `WORDCHARS`/`ICONV`, keeps curly apostrophes in words, trims edge quotes, and skips URLs and emails
- `is_correct` requires a single whitespace-delimited token with a letter or digit; empty and punctuation-only inputs are false
- Linux also searches XDG hunspell dirs and `/app/share/hunspell` (Flatpak) for dictionaries

## [0.4.0] - 2026-08-25

### Breaking
- `Error` is now `InvalidLocale` / `UnsupportedLocale` / `DictionaryNotFound` / `InitializationFailed` (removed `Unavailable`)
- macOS `with_locale` returns `UnsupportedLocale` if the language is not installed
- Added `Checker::locale` and `Checker::available_locales`
- Removed the crate binary (`src/main.rs`); use `examples/`

### Changed
- Crate-level rustdoc and docs on `Error`, `locale`, `available_locales`, and `range`
- Document platform defaults for `Checker::new()`, locale failure, suggestions cap, and UTF-8 error ranges in rustdoc
- README now states that macOS `Checker::new()` uses the system language
- `Checker` is no longer `Send`/`Sync`
- `Checker::new()` uses the environment/user locale on Linux and Windows when possible

### Fixed
- Linux `ignore` no longer panics when the word contains an interior NUL

## [0.3.0] - 2026-08-07

### Breaking
- Renamed the crate from `spellbound` to `spellkit`

### Changed
- Document locale and threading caveats in the README
- Drop obsolete `unexpected_cfgs` lint (no longer needed after objc2)

## [0.2.1] - 2026-08-07

### Added
- `Checker::is_correct`

### Changed
- Windows backend now uses the official `windows` crate instead of `winapi`
- Bump `cfg-if` to 1.x
- macOS backend now uses `objc2` / `objc2-app-kit` instead of `cocoa` / `objc`
- `Checker::check` now takes `&self` instead of `&mut self`
- Bump to Edition 2024 / MSRV 1.85.

### Fixed
- Pass null-terminated dictionary paths to Hunspell on Linux (stops spurious
  `cannot open …aff` stderr noise)

## [0.2.0] - 2026-08-06

### Breaking
- `Checker::new()` now returns `Result<Self, Error>` instead of panicking when
  the spellchecker (or hunspell dictionary) is unavailable
- `SpellingError` now includes UTF-8 byte `start` / `end` offsets into the
  checked text (in addition to `text()`)

### Added
- `Error` / `Error::Unavailable`
- Broader Linux dictionary search paths and `en_US` / `en_GB` fallbacks
- Word tokenization with offsets on Unix (alphabetic / `'` runs)
- GitHub Actions CI (Linux, macOS, Windows; fmt; clippy)
- README documentation for the maintained fork and Linux hunspell setup
- `Checker::with_locale` (`en_US` / `en-US` both accepted)
- `Checker::suggest` for spelling suggestions (capped at 10)

### Changed
- Edition 2021; dropped `lazy_static` / `extern crate`
- macOS serializes access to the shared `NSSpellChecker` with a `Mutex`
- Windows COM failures map to `Error` where creating the checker; UTF-16
  indices convert to UTF-8 byte ranges
- `hunspell-sys` 0.1.3 → 0.3.1 on Linux

### Removed
- Travis CI and AppVeyor configs

## [0.1.1] - 2020-02-05

Last release from upstream (`euclio/spellbound`).