//! Runs every ROM in ratchet.txt in parallel and reports the pass rate.
//!
//!   cargo run -p gb-tests --release --bin ratchet            # report
//!   cargo run -p gb-tests --release --bin ratchet -- --update # rewrite ratchet.txt
//!   cargo run -p gb-tests --release --bin ratchet -- timer    # filter by substring
//!
//! `--update` only ever records reality; review the diff before committing.
//! New ROMs found under roms/ are appended.

use gb_tests::{load_ratchet, ratchet_path, run_rom, tracked_roms, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let update = args.iter().any(|a| a == "--update");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let filter: Option<&String> = args.iter().find(|a| !a.starts_with('-'));

    let mut entries: Vec<(String, Option<Status>)> = load_ratchet()
        .into_iter()
        .map(|e| (e.rom, Some(e.expected)))
        .collect();
    for rom in tracked_roms() {
        if !entries.iter().any(|(r, _)| *r == rom) {
            entries.push((rom, None));
        }
    }

    let selected: Vec<usize> = (0..entries.len())
        .filter(|&i| filter.is_none_or(|f| entries[i].0.contains(f.as_str())))
        .collect();

    let results: Mutex<Vec<Option<(Status, String)>>> = Mutex::new(vec![None; entries.len()]);
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = selected.get(k) else { break };
                let r = run_rom(&entries[i].0);
                results.lock().unwrap()[i] = Some((r.status, r.detail));
            });
        }
    });
    let results = results.into_inner().unwrap();

    let (mut pass, mut total, mut regressions, mut improvements) = (0, 0, 0, 0);
    for &i in &selected {
        let (rom, expected) = &entries[i];
        let (actual, detail) = results[i].as_ref().unwrap();
        total += 1;
        if *actual == Status::Pass {
            pass += 1;
        }
        let tag = match (expected, actual) {
            (Some(Status::Pass), Status::Fail) => {
                regressions += 1;
                "REGRESSED"
            }
            (Some(Status::Fail) | None, Status::Pass) => {
                improvements += 1;
                "NEW PASS"
            }
            (None, Status::Fail) => "new fail",
            (_, Status::Pass) => "pass",
            (_, Status::Fail) => "fail",
        };
        println!("{tag:>9}  {rom}");
        if verbose || (*actual == Status::Fail && tag != "fail") {
            for line in detail.trim().lines().take(6) {
                println!("           | {line}");
            }
        }
    }
    println!(
        "\n{pass}/{total} passing ({:.1}%), {improvements} newly passing, {regressions} regressed",
        100.0 * pass as f64 / total.max(1) as f64
    );

    if update {
        let mut text = String::from(HEADER);
        for (i, (rom, expected)) in entries.iter().enumerate() {
            let status = match &results[i] {
                Some((s, _)) => *s,
                None => expected.unwrap_or(Status::Fail),
            };
            text.push_str(&format!("{} {rom}\n", status.as_str()));
        }
        std::fs::write(ratchet_path(), text).unwrap();
        println!("wrote {}", ratchet_path().display());
    }

    if regressions > 0 && !update {
        std::process::exit(1);
    }
}

const HEADER: &str = "\
# Pass-rate ratchet. One line per ROM: `<pass|fail> <path under gb-tests/roms/>`.
# `cargo test -p gb-tests` asserts reality matches this file exactly, so a
# regression fails the build, and so does a new pass that hasn't been recorded.
# Record progress with: cargo run -p gb-tests --release --bin ratchet -- --update
# Oracle is chosen by directory: mooneye/ = LD B,B register fingerprint,
# everything else = blargg serial text or cart-RAM signature.
";
