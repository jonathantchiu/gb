//! Runs every ROM in ratchet.txt in parallel and reports the pass rate.
//!
//!   cargo run -p gb-tests --release --bin ratchet            # report
//!   cargo run -p gb-tests --release --bin ratchet -- --update # rewrite ratchet.txt
//!   cargo run -p gb-tests --release --bin ratchet -- timer    # filter by substring
//!
//! `--update` only ever records reality; review the diff before committing.
//! New ROMs found under roms/ are appended.

use gb_tests::{load_ratchet, ratchet_path, run_rom_with_budget, tracked_roms, Denominator, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let update = args.iter().any(|a| a == "--update");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let filter: Option<&String> = args.iter().find(|a| !a.starts_with('-'));

    let mut entries: Vec<(String, Option<Status>, String, Option<u64>)> = load_ratchet()
        .into_iter()
        .map(|e| (e.rom, Some(e.expected), e.reason, e.timeout_mcycles))
        .collect();
    for rom in tracked_roms() {
        if !entries.iter().any(|(r, ..)| *r == rom) {
            entries.push((rom, None, String::new(), None));
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
                let r = run_rom_with_budget(&entries[i].0, entries[i].3);
                results.lock().unwrap()[i] = Some((r.status, format!("[{} M-cycles]\n{}", r.mcycles, r.detail)));
            });
        }
    });
    let results = results.into_inner().unwrap();

    let (mut pass, mut total, mut regressions, mut improvements, mut n_a) = (0, 0, 0, 0, 0);
    for &i in &selected {
        let (rom, expected, _, _) = &entries[i];
        let (actual, detail) = results[i].as_ref().unwrap();
        if expected.counts_for_denominator() {
            total += 1;
        } else {
            n_a += 1;
        }
        if *actual == Status::Pass {
            pass += 1;
        }
        let tag = match (expected, actual) {
            (Some(Status::Pass), a) if *a != Status::Pass => {
                regressions += 1;
                "REGRESSED"
            }
            (Some(e), a) if !e.counts_for_denominator() && a.counts_for_denominator() => {
                improvements += 1;
                "NEW PASS"
            }
            (Some(e), a) if e.counts_for_denominator() && *e != Status::Pass && *a == Status::Pass => {
                improvements += 1;
                "NEW PASS"
            }
            (None, a) if a.counts_for_denominator() => "new",
            (Some(e), _a) if !e.counts_for_denominator() => "n/a",
            (_, Status::Pass) => "pass",
            (_, Status::Timeout) => "timeout",
            (_, Status::Fail) => "fail",
            (_, Status::NotApplicable) => "n/a",
        };
        println!("{tag:>9}  {rom}");
        let show = verbose || (*actual != Status::Pass && !matches!(tag, "fail" | "timeout" | "n/a"));
        if show {
            for line in detail.trim().lines().take(6) {
                println!("           | {line}");
            }
        }
    }
    println!(
        "\n{pass}/{total} passing ({:.1}%), {n_a} not-applicable, {improvements} newly passing, {regressions} regressed",
        100.0 * pass as f64 / total.max(1) as f64
    );

    if update {
        let mut text = String::from(HEADER);
        for (i, (rom, expected, reason, tmo)) in entries.iter().enumerate() {
            let status = match &results[i] {
                Some((s, _)) => *s,
                None => expected.unwrap_or(Status::Fail),
            };
            // keep a recorded reason for passes; not-applicable keeps its line
            let why = if status == Status::Pass {
                if reason.is_empty() { " # (recorded by --update; review)" } else { "" }
            } else if !reason.is_empty() && expected.is_some_and(|e| !e.counts_for_denominator()) {
                " # "
            } else { "" };
            let r = if why.is_empty() && reason.is_empty() { String::new() } else if why.is_empty() { format!(" # {reason}") } else { why.to_string() };
            let t = tmo.map(|c| format!("@{c}")).unwrap_or_default();
            text.push_str(&format!("{} {rom}{t}{r}\n", status.as_str()));
        }
        std::fs::write(ratchet_path(), text).unwrap();
        println!("wrote {}", ratchet_path().display());
    }

    if regressions > 0 && !update {
        std::process::exit(1);
    }
}

const HEADER: &str = "\
# Pass-rate ratchet. One line per ROM:
#   <pass|fail|not-applicable|timeout> <path under gb-tests/roms/>[@Mcycles] [# reason]
# `cargo test -p gb-tests` asserts reality matches this file exactly, so a
# regression fails the build, and so does a new pass that hasn't been recorded.
# A `pass` line must carry a ` # reason` (anti-goal: unexplained passes are bugs).
# `not-applicable` = wrong hardware revision or needs a boot ROM not in CI, and
# leaves the pass-rate denominator. Record progress with:
#   cargo run -p gb-tests --release --bin ratchet -- --update
# Oracle by directory: mooneye/ = LD B,B register fingerprint; dmg-acid2 =
# framebuffer vs decoded upstream reference PNG; everything else = blargg
# serial text or cart-RAM signature. Budgets are emulated M-cycles, not wall time.
";
