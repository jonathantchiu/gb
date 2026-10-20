//! One test per line of `ratchet.txt`. Each asserts the ROM's actual result
//! matches the ratchet: a regression fails, and so does an unrecorded pass.

use gb_tests::{expected_status, run_rom, Status};

fn check(rom: &str) {
    let expected = expected_status(rom);
    let result = run_rom(rom);
    if result.status != expected {
        let hint = match result.status {
            Status::Pass => "newly passing: flip it to `pass` in ratchet.txt",
            Status::Fail => "REGRESSION",
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
