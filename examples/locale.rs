fn main() -> Result<(), spellkit::Error> {
    println!("available: {:?}", spellkit::Checker::available_locales());
    let c = spellkit::Checker::new()?;
    println!("locale: {}", c.locale());
    Ok(())
}
