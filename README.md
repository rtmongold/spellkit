# Spellkit

Cross-platform native spell checking for Rust.

[![On crates.io](https://img.shields.io/crates/v/spellkit.svg)](https://crates.io/crates/spellkit)
![Downloads](https://img.shields.io/crates/d/spellkit?style=flat-square)
[![CI](https://github.com/rtmongold/spellkit/actions/workflows/ci.yml/badge.svg)](https://github.com/rtmongold/spellkit/actions/workflows/ci.yml)
[![Docs](https://docs.rs/spellkit/badge.svg)](https://docs.rs/spellkit)

## Why Spellkit?

Use the spell-checking facilities already on the user's system.

| Platform | Backend |
| -------- | ------- |
| macOS    | [`NSSpellChecker`] |
| Windows  | [`ISpellChecker`] (Windows Spell Checker) |
| Linux / other Unix | [`Hunspell`] with system dictionaries |

[`ISpellChecker`]: https://docs.microsoft.com/en-us/windows/desktop/api/spellcheck/nn-spellcheck-ispellchecker
[`NSSpellChecker`]: https://developer.apple.com/documentation/appkit/nsspellchecker
[`Hunspell`]: https://hunspell.github.io/

Applications should not reimplement macOS, Windows, and Hunspell separately. Spellkit is one small API over those backends.

This project is **based on** [euclio/spellbound](https://github.com/euclio/spellbound) (last upstream commit 2020).

## What Spellkit is not

Spellkit does **not** bundle dictionaries or implement its own spelling algorithm. It wraps the platform backend and uses system / installed dictionaries. Behavior can differ across operating systems where the APIs differ.

That is the distinction from crates that ship an engine and word lists (for example Spellbook).

## Quick start

    cargo add spellkit

```rust
use spellkit::Checker;

fn main() -> Result<(), spellkit::Error> {
    let checker = Checker::new()?;

    for err in checker.check("I havv a spelling error.") {
        println!("{} @ {}..{}", err.text(), err.start(), err.end());
        for suggestion in checker.suggest(err.text()) {
            println!("  → {suggestion}");
        }
    }
    Ok(())
}
```

`Checker::locale()` is the language this instance is using. `Checker::available_locales()` lists what the OS can check.

```rust
use spellkit::Checker;

fn main() -> Result<(), spellkit::Error> {
    println!("available: {:?}", Checker::available_locales());
    let checker = Checker::new()?;
    println!("using: {}", checker.locale());
    Ok(())
}
```

## Features

- Cross-platform: macOS, Windows, Linux
- System dictionaries (no files shipped in the crate)
- Suggestions (up to 10)
- Locale via `with_locale` (`en_US` and `en-US` both work)
- Temporary ignored words (`ignore` is per checker, not global)
- UTF-8 byte ranges (`start` / `end` / `range`)
- Small API

## Platform support

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Backend | Hunspell | NSSpellChecker | ISpellChecker |
| `Checker::new()` | `LC_ALL` / `LC_MESSAGES` / `LANG` if a dict exists, else `en_US` / `en_GB` | system language | user locale, else `en-US` |
| Unknown `with_locale` | `Error::DictionaryNotFound` (paths searched) | `Error::UnsupportedLocale` | `Error::UnsupportedLocale` |
| Empty locale | `Error::InvalidLocale` | `Error::InvalidLocale` | `Error::InvalidLocale` |
| Suggestions | yes | yes | yes |
| `ignore` | yes (this handle only) | yes (this document tag only) | yes (this checker only) |
| `available_locales` | `*.dic` stems on disk (`DICPATH` then system dirs) | `availableLanguages` | `SupportedLanguages` |
| `Send` / `Sync` | no | no | no |

Linux also honors `DICPATH` (colon-separated directories) before `/usr/share/hunspell` and the other built-in paths.

Word breaks are **not** identical: Linux tokenizes alphanumeric / `'` runs; macOS and Windows use the OS checker.

## How it works

`Checker` is a thin wrapper. On each OS it calls the native API, then converts misspelling ranges to UTF-8 byte offsets into the original `&str`.

## Spellkit vs other approaches

**Why not Spellbook?** Use Spellbook when you want a portable engine and bundled (or app-shipped) dictionaries. Use Spellkit when you want the OS dictionaries, native suggestions, and minimal integration.

**Why not Hunspell directly?** You would own dictionary discovery, FFI, and a second implementation for macOS and Windows. Spellkit is that integration.

**Why not ispell?** You would own an external process, its lifetime, and the command protocol. Spellkit stays in-process.

## Errors

- empty locale → `Error::InvalidLocale`
- Linux missing `.aff`/`.dic` → `Error::DictionaryNotFound` (includes search paths)
- macOS / Windows language not installed → `Error::UnsupportedLocale`
- backend failed to start (null Hunspell handle, COM factory, empty macOS language) → `Error::InitializationFailed`

## Linux packages

- Arch: `pacman -S hunspell hunspell-en_us`
- Debian/Ubuntu: `apt install libhunspell-dev hunspell-en-us`
- Extra languages used in CI: `hunspell-de-de`, `hunspell-fr`

Without a dictionary, `Checker::new()` returns `Error::DictionaryNotFound`.

## Examples

    cargo run --example check -- "I havv a spelling error."
    cargo run --example suggestions -- "I beleeve I can fly"
    cargo run --example locale
    cargo run --example highlight

## Threading

`Checker` is not `Send` or `Sync`. Do not share it across threads. macOS also serializes access to the shared `NSSpellChecker`.

## Documentation

- [docs.rs/spellkit](https://docs.rs/spellkit)
- [CHANGELOG.md](CHANGELOG.md)

## Contributing

Issues and PRs: [github.com/rtmongold/spellkit](https://github.com/rtmongold/spellkit)

## License

MIT OR Apache-2.0

## Credits

Originally by [Andy Russell](https://github.com/euclio). Maintained as `spellkit` by Robert Mongold.