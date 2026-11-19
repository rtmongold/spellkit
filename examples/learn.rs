//! Persistent user dictionary: learn → survives recreate → unlearn.
//!
//! Run: `cargo run --example learn`
//!
//! On macOS/Windows this writes the OS user dictionary. On Linux it uses
//! `$XDG_DATA_HOME/spellkit/<locale>.words` (or `~/.local/share/spellkit/...`).

use spellkit::Checker;

fn main() {
    let word = "spellkitlearnwordxyz";

    let mut checker = match Checker::with_locale("en_US") {
        Ok(c) => c,
        Err(err) => {
            eprintln!("spellcheck unavailable: {err}");
            return;
        }
    };

    // Clean up a prior run if needed.
    let _ = checker.unlearn(word);

    println!(
        "before learn: has_learned={} check_count={}",
        checker.has_learned(word),
        checker.check(word).count()
    );

    if let Err(err) = checker.learn(word) {
        eprintln!("learn failed: {err}");
        return;
    }

    println!(
        "after learn: has_learned={} check_count={}",
        checker.has_learned(word),
        checker.check(word).count()
    );

    let locale = checker.locale().to_owned();
    drop(checker);

    // New checker — learned word should still be known.
    let mut checker = match Checker::with_locale(&locale) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("recreate failed: {err}");
            return;
        }
    };

    println!(
        "after recreate: has_learned={} check_count={}",
        checker.has_learned(word),
        checker.check(word).count()
    );

    if let Err(err) = checker.unlearn(word) {
        eprintln!("unlearn failed: {err}");
        return;
    }

    println!(
        "after unlearn: has_learned={} check_count={}",
        checker.has_learned(word),
        checker.check(word).count()
    );
}
