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
        // Rom paths may contain spaces (e.g. "04-op r,imm.gb"); strip any
        // `; reason` and `timeout=<n>` suffix, then take the rest as the path.
        let head = line.split(';').next().unwrap_or("").trim();
        let mut iter = head.splitn(2, char::is_whitespace);
        let _status = iter.next();
        let rest = iter.next().map(str::trim).unwrap_or("");
        let rom = rest
            .split(" timeout=")
            .next()
            .unwrap_or("")
            .trim();
        let mut name: String = rom
            .trim_end_matches(".gbc")
            .trim_end_matches(".gb")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        while name.contains("__") {
            name = name.replace("__", "_");
        }
        let name = name.trim_matches('_').to_string();
        assert!(seen.insert(name.clone()), "duplicate test name {name}");
        if rom.starts_with("dmg_sound/") || rom.starts_with("cgb_sound/") {
            writeln!(out, "#[test]\n#[cfg(feature = \"slow\")]\nfn {name}() {{ check({rom:?}); }}").unwrap();
        } else {
            writeln!(out, "#[test]\nfn {name}() {{ check({rom:?}); }}").unwrap();
        }
    }

    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("rom_tests.rs");
    std::fs::write(dest, out).unwrap();
}
