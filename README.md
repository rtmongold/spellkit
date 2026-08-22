# spellkit

[![On crates.io](https://img.shields.io/crates/v/spellkit.svg)](https://crates.io/crates/spellkit)
![Downloads](https://img.shields.io/crates/d/spellkit?style=flat-square)
[![CI](https://github.com/rtmongold/spellkit/actions/workflows/ci.yml/badge.svg)](https://github.com/rtmongold/spellkit/actions/workflows/ci.yml)
[![Docs](https://docs.rs/spellkit/badge.svg)](https://docs.rs/spellkit)

Native spell checking with a small Rust API.

This project is **based on** [euclio/spellbound](https://github.com/euclio/spellbound)
(last upstream commit 2020).

| Platform | API                |
| -------- | ------------------ |
| macOS    | [`NSSpellChecker`] |
| Windows  | [`ISpellChecker`]  |
| *nix     | [`hunspell`] |

[`ISpellChecker`]: https://docs.microsoft.com/en-us/windows/desktop/api/spellcheck/nn-spellcheck-ispellchecker
[`NSSpellChecker`]: https://developer.apple.com/documentation/appkit/nsspellchecker
[`hunspell`]: https://hunspell.github.io/

## Example

```rust
use spellkit::Checker;

fn main() -> Result<(), spellkit::Error> {
    let checker = Checker::new()?;
    // Or: Checker::with_locale("en-US")?;

    for err in checker.check("I beleeve I can fly") {
        println!("{} @ {}..{}", err.text(), err.start(), err.end());
        for suggestion in checker.suggest(err.text()) {
            println!("  → {suggestion}");
        }
    }
    Ok(())
}
```

`Checker::new()` uses a platform default: system language on macOS, `en-US` on Windows, and the first available of `en_US` / `en_GB` on Linux. Use `with_locale` for another language.

Unknown or unsupported locales behave differently by platform:

- **Linux:** missing dictionary / unknown locale → `Error::Unavailable`
- **macOS:** empty locale → `Error::Unavailable`; unknown tags may still create a checker (system fallback)
- **Windows:** unsupported language tag → `Error::Unavailable`

## Threading

macOS serializes access to the shared `NSSpellChecker`. Do not assume `Checker` is `Send` / `Sync` across platforms.

## Linux

Needs a hunspell dictionary on disk (default search includes `/usr/share/hunspell`). Example:

- Arch: `pacman -S hunspell hunspell-en_us`
- Debian/Ubuntu: `apt install libhunspell-dev hunspell-en-us`

Without a dictionary, `Checker::new()` returns `Error::Unavailable`.

## License
MIT OR Apache-2.0

## Credits

Originally by [Andy Russell](https://github.com/euclio). Maintained as `spellkit` by Robert Mongold.
