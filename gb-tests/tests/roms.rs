//! One test per line of `ratchet.txt`. Each asserts the ROM's actual result
//! matches the ratchet: a regression fails, and so does an unrecorded pass.

use gb_tests::{load_ratchet, run_rom_with_budget, Status};

fn entry_for(rom: &str) -> gb_tests::RatchetEntry {
    load_ratchet()
        .into_iter()
        .find(|e| e.rom == rom)
        .unwrap_or_else(|| panic!("{rom} not in ratchet.txt"))
}

fn check(rom: &str) {
    let entry = entry_for(rom);
    let expected = entry.expected;
    if expected == Status::NotApplicable {
        return; // wrong revision / needs a boot ROM not present in CI
    }
    let result = run_rom_with_budget(rom, entry.timeout_mcycles);
    if result.status != expected {
        let hint = match (expected, result.status) {
            (_, Status::Pass) => "newly passing: flip to `pass` with a reason in ratchet.txt",
            (Status::Pass, _) => "REGRESSION",
            (_, Status::Timeout) => "timed out (raise @Mcycles or fix the hang)",
            _ => "state mismatch",
        };
        panic!(
            "{rom}: ratchet={} actual={} ({hint})\n{}",
            expected.as_str(),
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
