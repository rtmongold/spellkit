fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let family = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if family.split(',').any(|family| family == "unix") && os != "macos" {
        pkg_config::Config::new()
            .probe("hunspell")
            .unwrap_or_else(|_| panic!("Failed to find hunspell via pkg-config"));
    }
}
