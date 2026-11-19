//! UI-thread ownership pattern: one Checker per locale, soft-fail, check/suggest/ignore.
//!
//! Run: `cargo run --example ui_integration`

use spellkit::Checker;
use std::cell::RefCell;
use std::ops::Range;

thread_local! {
    static SPELL: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

enum Slot {
    Ready {
        locale: Option<String>,
        checker: Checker,
    },
    Failed {
        locale: Option<String>,
    },
}

fn ensure_checker(locale: Option<&str>) -> bool {
    SPELL.with(|slot| {
        let mut slot = slot.borrow_mut();
        let needs_new = match slot.as_ref() {
            None => true,
            Some(Slot::Ready { locale: cached, .. }) | Some(Slot::Failed { locale: cached }) => {
                cached.as_deref() != locale
            }
        };
        if !needs_new {
            return matches!(slot.as_ref(), Some(Slot::Ready { .. }));
        }
        let result = match locale {
            Some(loc) => Checker::with_locale(loc),
            None => Checker::new(),
        };
        match result {
            Ok(checker) => {
                *slot = Some(Slot::Ready {
                    locale: locale.map(str::to_owned),
                    checker,
                });
                true
            }
            Err(err) => {
                eprintln!("spellcheck unavailable: {err}");
                *slot = Some(Slot::Failed {
                    locale: locale.map(str::to_owned),
                });
                false
            }
        }
    })
}

fn diagnostics(text: &str) -> Vec<(Range<usize>, String, Vec<String>)> {
    SPELL.with(|slot| {
        let slot = slot.borrow();
        let Some(Slot::Ready { checker, .. }) = slot.as_ref() else {
            return Vec::new();
        };
        checker
            .check(text)
            .map(|err| {
                debug_assert!(text.is_char_boundary(err.start()));
                debug_assert!(text.is_char_boundary(err.end()));
                debug_assert_eq!(&text[err.range()], err.text());
                let suggestions = checker.suggest(err.text());
                (err.range(), err.text().to_owned(), suggestions)
            })
            .collect()
    })
}

fn ignore_word(word: &str) {
    SPELL.with(|slot| {
        if let Some(Slot::Ready { checker, .. }) = slot.borrow_mut().as_mut() {
            checker.ignore(word);
        }
    });
}

fn print_diags(label: &str, text: &str) {
    println!("{label}");
    let diags = diagnostics(text);
    if diags.is_empty() {
        println!(" (no misspellings)");
        return;
    }
    for (range, word, suggestions) in diags {
        println!(" {word} @ {range:?}");
        for s in suggestions {
            println!("    → {s}");
        }
    }
}

fn main() {
    // Soft-fail: missing dict must not abort.
    if !ensure_checker(Some("en_US")) && !ensure_checker(None) {
        return;
    }

    let text = "I beleeve I can fly";
    print_diags("before ignore:", text);

    ignore_word("beleeve");
    print_diags("after ignore(\"beleeve\"):", text);

    // Revision-token sketch for debounce / owning worker (Checker never crosses threads):
    // let mut rev = 0u64;
    // on each keystroke: rev += 1; let this = rev; send (this, text.to_owned()) to worker;
    // worker replies (this, ranges); UI applies only if this == rev (drop stale).
    // Create Checker inside the worker thread; send String / ranges on the channel, not Checker.
    println!("hint: gate async results with a revision counter; keep Checker on one thread");
}
