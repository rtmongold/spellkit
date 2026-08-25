use spellkit::Checker;
use std::env;

fn main() -> Result<(), spellkit::Error> {
    let text = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let checker = Checker::new()?;
    for error in checker.check(&text) {
        println!("{}", error.text());
    }
    Ok(())
}
