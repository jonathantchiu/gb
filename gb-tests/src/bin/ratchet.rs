//! Runs every ROM in ratchet.txt in parallel and reports the pass rate.
//!
//!   cargo run -p gb-tests --release --bin ratchet            # report
//!   cargo run -p gb-tests --release --bin ratchet -- --update # rewrite ratchet.txt
//!   cargo run -p gb-tests --release --bin ratchet -- timer    # filter by substring
//!
//! `--update` only ever records reality; review the diff before committing.
//! New ROMs found under roms/ are appended. Not-applicable ROMs are skipped
//! and excluded from the denominator. Every fail->pass flip must be given a
//! one-line `; reason` in ratchet.txt (enforced here and in review).

use gb_tests::{budget_for, load_ratchet, ratchet_path, run_rom, tracked_roms, RatchetEntry, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let update = args.iter().any(|a| a == "--update");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let filter: Option<&String> = args.iter().find(|a| !a.starts_with('-'));

    let mut entries: Vec<RatchetEntry> = load_ratchet();
    for rom in tracked_roms() {
        if !entries.iter().any(|e| e.rom == rom) {
            entries.push(RatchetEntry {
                rom,
                expected: Status::Timeout,
                reason: None,
                timeout_mcycles: None,
            });
        }
    }

    let selected: Vec<usize> = (0..entries.len())
        .filter(|&i| filter.is_none_or(|f| entries[i].rom.contains(f.as_str())))
        .collect();

    let results: Mutex<Vec<Option<(Status, String)>>> = Mutex::new(vec![None; entries.len()]);
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = selected.get(k) else { break };
                let e = &entries[i];
                if e.expected == Status::NotApplicable {
                    results.lock().unwrap()[i] = Some((Status::NotApplicable, "skipped".to_string()));
                    continue;
                }
                let r = run_rom(&e.rom, budget_for(e));
                results.lock().unwrap()[i] = Some((
                    r.status,
                    format!("[{} M-cycles, {:#018x}]\n{}", r.mcycles, r.hash, r.detail),
                ));
            });
        }
    });
    let results = results.into_inner().unwrap();

    let (mut pass, mut applicable, mut regressions, mut improvements, mut new_reason_needed) =
        (0, 0, 0, 0, 0);
    for &i in &selected {
        let entry = &entries[i];
        let rom = &entry.rom;
        let expected = Some(entry.expected);
        let (actual, detail) = results[i].as_ref().unwrap();
        if *actual != Status::NotApplicable {
            applicable += 1;
            if *actual == Status::Pass {
                pass += 1;
            }
        }
        let tag = match (expected, actual) {
            (Some(Status::Pass), s) if *s != Status::Pass && *s != Status::NotApplicable => {
                regressions += 1;
                "REGRESSED"
            }
            (Some(Status::Fail) | Some(Status::Timeout) | None, Status::Pass) => {
                improvements += 1;
                if entry.reason.is_none() {
                    new_reason_needed += 1;
                }
                "NEW PASS"
            }
            (None, Status::Fail) => "new fail",
            (None, Status::Timeout) => "new timeout",
            (_, Status::Pass) => "pass",
            (_, Status::Fail) => "fail",
            (_, Status::Timeout) => "timeout",
            (_, Status::NotApplicable) => "not-applicable",
        };
        println!("{tag:>9}  {rom}");
        if verbose || (*actual == Status::Fail && tag != "fail") {
            for line in detail.trim().lines().take(6) {
                println!("           | {line}");
            }
        }
    }
    println!(
        "\n{pass}/{applicable} passing ({:.1}%), {improvements} newly passing, {regressions} regressed",
        100.0 * pass as f64 / applicable.max(1) as f64
    );
    if new_reason_needed > 0 {
        println!("note: {new_reason_needed} newly-passing ROM(s) need a `; reason` in ratchet.txt");
    }

    if update {
        let mut text = String::from(HEADER);
        for (i, entry) in entries.iter().enumerate() {
            let status = match &results[i] {
                Some((s, _)) => *s,
                None => entry.expected,
            };
            // Preserve existing status for ROMs not selected by the filter.
            let status = if results[i].is_none() { entry.expected } else { status };
            let mut line = format!("{} {}", status.as_str(), entry.rom);
            if let Some(t) = entry.timeout_mcycles {
                line += &format!(" timeout={t}");
            }
            if let Some(r) = &entry.reason {
                line += &format!(" ; {r}");
            } else if status == Status::Pass && entry.expected != Status::Pass {
                line += " ; TODO: explain why this passes";
            }
            text.push_str(&line);
            text.push('\n');
        }
        std::fs::write(ratchet_path(), text).unwrap();
        println!("wrote {}", ratchet_path().display());
    }

    if regressions > 0 && !update {
        std::process::exit(1);
    }
}

const HEADER: &str = "\
# Pass-rate ratchet. One line per ROM: `<status> <path under gb-tests/roms/> [timeout=<M-cycles>] [; reason]`.
# Status is one of: pass | fail | not-applicable | timeout.
# not-applicable = wrong hardware revision or needs the real boot ROM in CI; excluded from the denominator.
# timeout is distinct from fail: the ROM never reported within its cycle budget.
# Every fail->pass flip must carry a one-line `; reason` in the same commit.
# `cargo test -p gb-tests` asserts reality matches this file exactly.
# Record progress with: cargo run -p gb-tests --release --bin ratchet -- --update
# Oracle: mooneye/ = LD B,B register fingerprint; dmg-acid2 = framebuffer vs decoded reference PNG; else blargg.
";
