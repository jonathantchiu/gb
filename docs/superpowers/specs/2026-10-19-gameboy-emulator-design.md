# Game Boy Emulator — Design

**Date:** 2026-10-19
**Status:** Awaiting approval

## Intent

Build a cycle-accurate Game Boy (DMG) and Game Boy Color (CGB) emulator in Rust, developed over months against the public hardware test ROM suites.

### Why this, why now

The owner wants a long-horizon personal project that genuinely resists being completed in a single sitting. Most project ideas fail this test: a toy database or toy compiler is "done" whenever the author declares it done, so progress is self-graded and the work collapses to a weekend.

Emulation does not have this problem. The Game Boy has an unusually rich ecosystem of hardware test ROMs — programs written by other people, run on real hardware, that report pass/fail on precise timing and edge-case behavior. These form an **external oracle the author does not control**. Pass rate is a number that starts low and climbs slowly, and it cannot be faked.

The expected shape of the work is the point: long single-bug investigations where the symptom (a test ROM prints `Failed #3`) is several layers removed from the cause (an interrupt is serviced one T-cycle too early during a HALT bug edge case). That is the kind of problem the owner wants to spend sessions on.

A naive implementation boots a commercial game in days. Passing `mooneye-gb`'s PPU timing tests takes months. The gap between those two points is the project.

## Goals

1. **Pass the public test suites.** Specifically, and in rough priority order:
   - `blargg` `cpu_instrs` (11 ROMs) — CPU instruction correctness
   - `blargg` `instr_timing` — instruction cycle counts
   - `blargg` `mem_timing` / `mem_timing-2` — memory access timing
   - `mooneye-test-suite` `acceptance/` — the hard one; timing of interrupts, PPU mode transitions, OAM DMA, timer behavior
   - `dmg-acid2` and `cgb-acid2` — PPU rendering correctness, verified by screenshot
   - `blargg` `dmg_sound` / `cgb_sound` — APU correctness
2. **Run real commercial games correctly**, including ones known to depend on hardware quirks (Prehistorik Man, Pinball Deluxe, Road Rash, Demotronic demo).
3. **Cycle accuracy at T-cycle granularity.** Components advance in lockstep with the CPU, not in per-instruction batches.
4. **Game Boy Color support** after DMG is solid: double-speed mode, HDMA, palettes, VRAM/WRAM banking.
5. **A test harness fast enough to run on every change**, so regressions are caught immediately.
6. **A ratchet.** Pass rate is recorded in a committed file. It is allowed to go up. A change that lowers it fails CI.

## Non-goals

These are explicitly out of scope. Some may become goals later; none should influence early architecture beyond leaving the door open.

- **Game Boy Advance.** Different CPU, different project.
- **Link cable / multiplayer / netplay.** The serial port is implemented only far enough to capture test ROM output.
- **A browser (`wasm32`) build.** Foreclosed by the CPU design decision below, deliberately.
- **Maximum performance.** Correctness wins every time. Optimization happens only when the test suite gets slow enough to hurt the development loop — see the suite runtime decision below.
- **A polished end-user application.** No installer, no ROM browser, no settings UI, no shader pipeline. A window, audio, and a keyboard is the whole frontend.
- **Rewind, netplay, cheats, achievements.** Save states are in scope only because they're useful for debugging.
- **Mobile or console ports.**

## Anti-goals

Things that would make this a failure *even if the tests pass*:

- **Special-casing to pass tests.** Any code shaped like "if the ROM title is X, do Y", or a hardcoded constant tuned until one test flipped green with no understanding of why, is a defect. The goal is a model of the hardware that happens to pass the tests, not a program that passes the tests.
- **A slow or unreliable test loop.** If running the suite takes minutes, or produces flaky results, the oracle stops being used and the project loses its spine.
- **Per-instruction timing batching.** Implementing the CPU as "execute instruction, then advance peripherals by N cycles" is faster to write and makes the hardest tests permanently unreachable. This is the single most common architectural dead end in hobby emulators, and recovering from it means a rewrite.
- **Unverified refactoring.** Any structural change must be bracketed by a full suite run before and after.

## Constraints

- **Language: Rust** (2021 edition or later). Chosen for performance adequate to cycle-accurate emulation, good test tooling.
- **No existing emulator crates.** Every hardware component is written from scratch. Third-party crates are fine for windowing, audio output, and CLI parsing — not for emulation logic.
- **Runs on a laptop.** No GPU requirement beyond a framebuffer blit.
- **Headless-first.** The core must be fully driveable and testable with no window, no audio device, and no real-time clock.
- **ROM legality.** Test ROMs are freely redistributable and are vendored or submoduled. Commercial game ROMs and Nintendo boot ROMs are the owner's own, loaded from gitignored local paths, and are never committed.

## Approach: decided by the owner

### First milestone is pass rate, not pixels

Development starts headless. CPU and memory bus come first, driven by `blargg/cpu_instrs`, which reports results over the serial port as ASCII — no PPU required to read them. Graphics work begins only once the CPU is provably correct.

This is slower to something visible and is the right call anyway: debugging a rendering bug on top of an unverified CPU means two unknowns at once, and that is where hobby emulator projects stall.

### Cycle-stepped, from the beginning

The CPU does not execute an instruction and then tell the rest of the system how much time passed. Instead, every memory access the CPU performs advances the entire system by 4 T-cycles, and peripherals observe the bus as it happens. The PPU, timer, APU, and DMA controller each step one T-cycle at a time.

This is more work up front and is non-negotiable, because retrofitting it later is a rewrite. See the anti-goals.

### CPU holds mid-instruction state on the call stack

The CPU is written as `cpu.step(&mut bus)`, where each bus access ticks the system 4 T-cycles inline:

```rust
fn read(&mut self, bus: &mut Bus, addr: u16) -> u8 {
    bus.tick(4);
    bus.read(addr)
}
```

Mid-instruction state lives in Rust locals and the call stack. This is fully cycle-accurate and passes every suite named in the goals.

The alternative — an explicit microcode state machine with all state in struct fields — was considered and rejected. It would allow mid-instruction save states, a single-T-cycle debugger, and a `wasm32` build, at roughly 2–3x the CPU code and roughly double the time to a correct CPU. None of those three capabilities is worth that, and the browser build is now a stated non-goal.

**Consequence the builder must accept:** save states and debugger stepping are at instruction boundaries only. Phase 7 debugging leans on trace logging rather than cycle-granular halting. This decision is effectively irreversible; changing it later means rewriting the CPU.

### Two clock domains from phase 2

In CGB double-speed mode the CPU, timer/DIV, and serial run at 2x while the PPU and APU base clocks do not. A single global tick is therefore false, and the bus carries two clock domains from phase 2 onward — built and unit-tested long before anything actually runs at 2x.

This means some early complexity and some dead code during the DMG phases. That cost is accepted deliberately: deferring it to phase 8 would be precisely the timing retrofit the anti-goals forbid, and it would touch every component.

### Target revisions: DMG-ABC and CGB-E, as separate cores

`mooneye`'s `acceptance/` tests are revision-specific and mutually exclusive — no single configuration passes all of them. The emulator claims to be two specific machines: **DMG-ABC** and **CGB-E**. Tests targeting other revisions (`-dmg0`, `-S`, `-cgb0`, …) are marked `not-applicable` in the ratchet and leave the denominator.

DMG and CGB are **parallel implementations sharing components**, not one core with a revision flag threaded through it. This keeps mode checks out of hot paths and keeps the ratchet denominator honest for each machine.

CGB-in-DMG-compatibility mode is an optional later goal, not a phase gate.

### Boot ROM: optional, user-supplied

The default path synthesizes correct post-boot register, palette, and I/O state directly — no boot ROM needed, no Nintendo copyright in the repo. When the owner supplies a real boot ROM at a gitignored local path, the emulator runs it instead.

`mooneye`'s `boot_regs` / `boot_hwio` / `boot_div` families therefore run locally and are marked `not-applicable` in CI.

## Approach: settled by the designer

These were not specified by the owner. They are recorded here as the designer's calls, open to correction.

### The acid2 oracle compares against the upstream reference image

`dmg-acid2` and `cgb-acid2` are checked by decoding the **upstream reference PNG** and comparing pixels — never by hashing our own first passing output, which would turn an external oracle into a regression lock on whatever we happened to produce.

This forces three things to be pinned in the harness and documented there: the canonical framebuffer format is RGBA8888; the DMG four-grey palette mapping is fixed; and CGB output is a raw RGB555→RGB888 expansion with **no** color-correction curve applied. Reference images differ depending on that last choice, so it has to be explicit or the test fails for the wrong reason.

### The ratchet has four states and requires a reason

Two states cannot describe reality. Each ROM is one of:

- `pass`
- `fail`
- `not-applicable` — wrong hardware revision, or requires a boot ROM in CI
- `timeout` — distinct from `fail`, because the causes differ

Per-ROM timeouts are expressed in emulated cycles, not wall time, so the suite is deterministic.

Every `fail`→`pass` flip must carry a one-line `reason` field in the same commit. This is the anti-goal about unexplained passes, made mechanical rather than aspirational.

### Suite runtime: a fast tier under 2 minutes

"Runs in seconds" is aspirational and collides with cycle-stepping — `cpu_instrs` alone is roughly 30 emulated seconds. The real bar: the **fast tier** stays under 2 minutes fully parallel and runs on every change. Slow ROMs (`dmg_sound`, `cgb_sound`, the longer mooneye cases) sit behind a feature flag and run before commits.

No dedicated performance workstream until the fast tier exceeds 3 minutes. At that point optimization is licensed, because the oracle being painful to run is an existential risk to the project.

`same-suite` is tracked in the ratchet as its own tier, outside the primary goals.

### Where modeling ends and tuning begins

Some required behaviors — exact mode-3 length penalties, OAM corruption, certain HALT/STOP edges — are known only empirically, and Pan Docs and AntonioND occasionally disagree without explaining a mechanism.

The acceptance rule: **a documented citation to a community hardware finding is sufficient justification, even with no mechanism.** An undocumented constant with no citation is not, and is an open bug regardless of what it makes pass.

### Commercial games are verified locally and never gate a phase

Goal 2 cannot run in CI, since those ROMs are never committed. The mechanism is a gitignored local ROM directory with a boot-to-N-frames framebuffer hash against locally stored goldens.

It never gates CI and never gates a phase. **Phases 3 and 4 exit on test ROM results only.** Commercial games are a signal, not an oracle.

## Shape of the system

A `Bus` owns the components and mediates all access. The CPU holds a reference to the bus and drives it. A tick on the bus advances PPU, timer, DMA, and APU within their respective clock domains and raises interrupt flags as a side effect.

Components:

- **CPU** — SM83 core. Instruction decoding, the interrupt service routine with its correct 5-M-cycle timing, HALT (including the HALT bug), and STOP.
- **MMU / Bus** — address decoding, memory-mapped I/O registers, VRAM and OAM access blocking during the PPU's busy modes, the boot ROM overlay, the two clock domains.
- **Cartridge** — a `Mapper` trait, with implementations added as games demand: no-MBC, MBC1 (including the multicart wiring quirk), MBC2, MBC3 (including RTC), MBC5.
- **PPU** — the pixel FIFO, not a scanline renderer. Background and sprite fetchers, mode transitions with correct timing including the variable mode-3 length, window behavior, and the LCD-off/on edge cases.
- **Timer** — DIV as a free-running internal counter, TIMA with its reload delay quirk, and the falling-edge detection that drives it.
- **APU** — four channels, frame sequencer, length/envelope/sweep units, resampled to the host rate.
- **Joypad**, **Serial** (output capture sufficient for test ROMs), **DMA** and **HDMA**.

## Test harness

The oracle is wired in as ordinary `cargo test`. Three kinds, matching how the ROMs report:

1. **Serial output.** `blargg` ROMs print results as text over the serial port. Run to completion or cycle timeout, compare captured output against the expected string.
2. **Magic register state.** `mooneye` ROMs signal pass/fail by executing a specific instruction sequence that leaves a known fingerprint in the CPU registers. Run until that sequence executes, then assert.
3. **Framebuffer comparison.** `acid2` and similar, against the decoded upstream reference image as described above.

Each ROM is a test case, so failures name exactly which hardware behavior broke.

## Crate layout

A Cargo workspace:

- `gb-core` — all emulation logic. No I/O, no windowing, no audio device. This is where essentially all the work happens.
- `gb-frontend` — window, input, audio output, CLI.
- `gb-tests` — the ROM harness and the ratchet.
- `gb-debugger` — later; disassembler, breakpoints, memory and VRAM viewers, execution tracing.

Keeping the core free of I/O is what makes the headless test suite fast and deterministic.

## Roadmap

Phases are ordered by dependency, not by schedule. Each ends at a verifiable oracle result.

1. **CPU core.** All 256 opcodes plus the CB-prefixed table, flags, interrupts, synthesized post-boot state. Ends when all 11 `cpu_instrs` ROMs pass.
2. **Timing.** Cycle-stepped bus, **two clock domains**, timer with its quirks. Ends at `instr_timing` and `mem_timing`.
3. **PPU.** Pixel FIFO, background, window, sprites. Ends at `dmg-acid2` matching the upstream reference image.
4. **Cartridge mappers.** MBC1/2/3/5, battery-backed saves. Ends at the mapper test ROMs.
5. **Frontend.** Window, input, audio device, instruction-boundary save states.
6. **APU.** Ends at `dmg_sound`.
7. **Mooneye acceptance, DMG-ABC.** The long tail. The bulk of the project's lifetime lives here.
8. **CGB-E core.** Double speed active, HDMA, color palettes, banking. Ends at `cgb-acid2` and `cgb_sound`.
9. **Debugger**, and whatever the owner wants next.

Phases 1–5 are a working emulator. Phase 7 is where it becomes a good one.

## Decisions left to the builder

Deliberately unspecified; a competent builder should settle these during planning:

- Frontend crate choices for windowing, pixel blitting, and audio output.
- How test ROMs are vendored (submodule vs. a fetch script), and the exact ratchet file format.
- CPU instruction dispatch strategy — generated tables, a large match, or a decoded-field approach. Any of these works with the call-stack design; pick for readability.
- Internal module boundaries within `gb-core`, and exactly which components the DMG and CGB cores share.
- Error handling conventions, logging, and the tracing format — noting that tracing carries more weight than usual, since it substitutes for cycle-granular debugging.
- Whether the PPU's pixel FIFO is modeled as a literal queue or an equivalent state machine, provided the observable timing is identical.
- CI provider and audio sync model.

## Reference material

- Pan Docs (`gbdev.io/pandocs`) — the primary hardware reference.
- The Cycle-Accurate Game Boy Docs (AntonioND) — timing diagrams; the authority for phases 2 and 3.
- `mooneye-test-suite`, `blargg`'s test ROMs, `dmg-acid2` / `cgb-acid2`, `same-suite`.
- The `gbdev` community Discord and the `awesome-gbdev` list.
