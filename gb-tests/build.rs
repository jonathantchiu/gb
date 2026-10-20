use std::fmt::Write;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let ratchet = Path::new(&manifest).join("ratchet.txt");
    println!("cargo:rerun-if-changed={}", ratchet.display());
    let text = std::fs::read_to_string(&ratchet).expect("ratchet.txt");

    let mut out = String::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (_, rom) = line.split_once(' ').expect("ratchet line");
        let rom = rom.trim();
        let mut name: String = rom
            .trim_end_matches(".gb")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        while name.contains("__") {
            name = name.replace("__", "_");
        }
        let name = name.trim_matches('_').to_string();
        assert!(seen.insert(name.clone()), "duplicate test name {name}");
        writeln!(out, "#[test]\nfn {name}() {{ check({rom:?}); }}").unwrap();
    }

    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("rom_tests.rs");
    std::fs::write(dest, out).unwrap();
}
