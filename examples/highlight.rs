use spellkit::Checker;

fn main() -> Result<(), spellkit::Error> {
    let text = "I beleeve I can fly";
    let checker = Checker::new()?;
    for err in checker.check(text) {
        let range = err.range();
        println!("{} @ {range:?}", err.text());
        println!("  slice: {}", &text[range]);
    }
    Ok(())
}
