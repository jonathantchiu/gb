//! One test per line of `ratchet.txt`. Each asserts the ROM's actual result
//! matches the ratchet: a regression fails, and so does an unrecorded pass.
//! `not-applicable` ROMs are skipped (wrong revision or boot-ROM-only).

use gb_tests::{budget_for, ratchet_entry, run_rom, Status};

fn is_slow(rom: &str) -> bool {
    rom.starts_with("dmg_sound/") || rom.starts_with("cgb_sound/")
}

fn check(rom: &str) {
    let entry = ratchet_entry(rom);
    if entry.expected == Status::NotApplicable {
        return;
    }
    if is_slow(rom) && !cfg!(feature = "slow") {
        return;
    }
    let result = run_rom(rom, budget_for(&entry));
    if result.status != entry.expected {
        let hint = match (entry.expected, result.status) {
            (_, Status::Pass) => "newly passing: flip to `pass` in ratchet.txt with a `; reason`",
            (Status::Pass, _) => "REGRESSION",
            _ => "record the new status in ratchet.txt",
        };
        panic!(
            "{rom}: ratchet={} actual={} ({hint})\n{}",
            entry.expected.as_str(),
            result.status.as_str(),
            result.detail
        );
    }
}

include!(concat!(env!("OUT_DIR"), "/rom_tests.rs"));

#[test]
fn every_vendored_rom_is_in_ratchet() {
    let listed: std::collections::HashSet<String> =
        gb_tests::load_ratchet().into_iter().map(|e| e.rom).collect();
    let mut missing = Vec::new();
    for rom in gb_tests::tracked_roms() {
        if !listed.contains(&rom) {
            missing.push(rom);
        }
    }
    assert!(missing.is_empty(), "ROMs not in ratchet.txt: {missing:#?}");
}
