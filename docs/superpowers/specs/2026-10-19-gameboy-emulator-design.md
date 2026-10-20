# Game Boy Emulator — Design

**Date:** 2026-10-19
**Status:** Approved design, ready for planning

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
5. **A test harness that runs the whole suite in seconds**, so correctness can be re-verified after every change, and regressions are caught immediately.
6. **A ratchet.** Pass rate is recorded in a committed file. It is allowed to go up. A change that lowers it fails CI.

## Non-goals

These are explicitly out of scope. Some may become goals later; none should influence early architecture beyond leaving the door open.

- **Game Boy Advance.** Different CPU, different project.
- **Link cable / multiplayer / netplay.** The serial port is implemented only far enough to capture test ROM output.
- **Maximum performance.** Correctness wins every time. A target of 5–10x realtime on a modern laptop is plenty; beyond that, don't optimize.
- **A polished end-user application.** No installer, no ROM browser, no settings UI, no shader pipeline. A window, audio, and a keyboard is the whole frontend.
- **Rewind, netplay, cheats, achievements.** Save states are in scope only because they're useful for debugging.
- **Mobile or console ports.**

## Anti-goals

Things that would make this a failure *even if the tests pass*:

- **Special-casing to pass tests.** Any code shaped like "if the ROM title is X, do Y", or a hardcoded constant tuned until one test flipped green with no understanding of why, is a defect. The goal is a model of the hardware that happens to pass the tests, not a program that passes the tests. When a test passes and the reason isn't understood, that is an open bug.
- **A slow or unreliable test loop.** If running the suite takes minutes, or produces flaky results, the oracle stops being used and the project loses its spine.
- **Per-instruction timing batching.** Implementing the CPU as "execute instruction, then advance peripherals by N cycles" is faster to write and makes the hardest tests permanently unreachable. This is the single most common architectural dead end in hobby emulators, and recovering from it means a rewrite.
- **Unverified refactoring.** Any structural change must be bracketed by a full suite run before and after.

## Constraints

- **Language: Rust** (2021 edition or later). Chosen for performance adequate to cycle-accurate emulation, good test tooling, and a plausible `wasm32` target later.
- **No existing emulator crates.** Every hardware component is written from scratch. Third-party crates are fine for windowing, audio output, and CLI parsing — not for emulation logic.
- **Runs on a laptop.** No GPU requirement beyond a framebuffer blit.
- **Headless-first.** The core must be fully driveable and testable with no window, no audio device, and no real-time clock.
- **ROM legality.** Test ROMs are freely redistributable and are vendored or submoduled. Commercial game ROMs are the owner's own and are never committed.

## Approach: decided

### First milestone is pass rate, not pixels

Development starts headless. CPU and memory bus come first, driven by `blargg/cpu_instrs`, which reports results over the serial port as ASCII — no PPU required to read them. Graphics work begins only once the CPU is provably correct.

This is slower to something visible and is the right call anyway: debugging a rendering bug on top of an unverified CPU means two unknowns at once, and that is where hobby emulator projects stall.

### Cycle-stepped, from the beginning

The CPU does not execute an instruction and then tell the rest of the system how much time passed. Instead, every memory access the CPU performs advances the entire system by 4 T-cycles, and peripherals observe the bus as it happens. The PPU, timer, APU, and DMA controller each step one T-cycle at a time.

This is more work up front and is non-negotiable, because retrofitting it later is a rewrite. See the anti-goals.

### Shape of the system

A `Bus` owns the components and mediates all access. The CPU holds a reference to the bus and drives it. A `tick()` on the bus advances PPU, timer, DMA, and APU by one T-cycle each and raises interrupt flags as a side effect.

Components:

- **CPU** — SM83 core. Instruction decoding, the interrupt service routine with its correct 5-M-cycle timing, HALT (including the HALT bug), and STOP.
- **MMU / Bus** — address decoding, memory-mapped I/O registers, VRAM and OAM access blocking during the PPU's busy modes, the boot ROM overlay.
- **Cartridge** — a `Mapper` trait, with implementations added as games demand: no-MBC, MBC1 (including the multicart wiring quirk), MBC2, MBC3 (including RTC), MBC5.
- **PPU** — the pixel FIFO, not a scanline renderer. Background and sprite fetchers, mode transitions with correct timing including the variable mode-3 length, window behavior, and the LCD-off/on edge cases.
- **Timer** — DIV as a free-running internal counter, TIMA with its reload delay quirk, and the falling-edge detection that drives it.
- **APU** — four channels, frame sequencer, length/envelope/sweep units, resampled to the host rate.
- **Joypad**, **Serial** (output capture sufficient for test ROMs), **DMA** and **HDMA**.

### Test harness

The oracle is wired in as ordinary `cargo test`, so the full suite runs on every change.

Three kinds of oracle, matching how the ROMs report:

1. **Serial output.** `blargg` ROMs print results as text over the serial port. Run to completion or timeout, compare captured output against the expected string.
2. **Magic register state.** `mooneye` ROMs signal pass/fail by executing a specific instruction sequence that leaves a known fingerprint in the CPU registers. Run until that sequence executes, then assert.
3. **Framebuffer hash.** `acid2` and similar render a reference image. Hash the framebuffer after a fixed number of frames and compare against a committed expected hash, with the reference PNG stored alongside for human inspection when it fails.

Each ROM is a test case, so failures name exactly which hardware behavior broke.

The **ratchet** is a committed file listing every ROM and its current expected status (`pass` / `fail`). The suite asserts reality matches the file. A ROM that newly passes is a one-line commit flipping it to `pass`; a ROM that newly fails is a build failure. This makes progress visible in version control and makes regressions impossible to miss.

### Crate layout

A Cargo workspace:

- `gb-core` — all emulation logic. No I/O, no windowing, no audio device, `no_std`-friendly in spirit if not in fact. This is where essentially all the work happens.
- `gb-frontend` — window, input, audio output, CLI.
- `gb-tests` — the ROM harness and the ratchet.
- `gb-debugger` — later; disassembler, breakpoints, memory and VRAM viewers, execution tracing.

Keeping the core free of I/O is what makes the headless test suite fast and deterministic.

### Roadmap

Phases are ordered by dependency, not by schedule. Each ends at a verifiable oracle result.

1. **CPU core.** All 256 opcodes plus the CB-prefixed table, flags, interrupts. Ends when all 11 `cpu_instrs` ROMs pass.
2. **Timing.** Cycle-stepped bus, timer with its quirks. Ends at `instr_timing` and `mem_timing`.
3. **PPU.** Pixel FIFO, background, window, sprites. Ends at `dmg-acid2` matching the reference image, and commercial games rendering correctly.
4. **Cartridge mappers.** MBC1/2/3/5, battery-backed saves. Ends with a broad library booting.
5. **Frontend.** Window, input, audio device, save states.
6. **APU.** Ends at `dmg_sound`.
7. **Mooneye acceptance.** The long tail. The bulk of the project's lifetime lives here.
8. **Game Boy Color.** Double speed, HDMA, color palettes, banking. Ends at `cgb-acid2` and `cgb_sound`.
9. **Debugger**, and whatever the owner wants next.

Phases 1–5 are a working emulator. Phase 7 is where it becomes a good one.

## Decisions left to the builder

The following are deliberately unspecified. A competent builder should settle them during planning:

- Frontend crate choices for windowing, pixel blitting, and audio output.
- How test ROMs are vendored (submodule vs. a fetch script), and the exact ratchet file format.
- CPU instruction dispatch strategy — generated tables, a large match, or a decoded-field approach. Any of these can be cycle-accurate; pick for readability and change it later if profiling demands.
- Internal module boundaries within `gb-core` beyond the components named above.
- Error handling conventions, logging, and the tracing format.
- Whether the PPU's pixel FIFO is modeled as a literal queue or an equivalent state machine, provided the observable timing is identical.

## Reference material

- Pan Docs (`gbdev.io/pandocs`) — the primary hardware reference.
- The Cycle-Accurate Game Boy Docs (AntonioND) — timing diagrams; the authority for phases 2 and 3.
- `mooneye-test-suite`, `blargg`'s test ROMs, `dmg-acid2` / `cgb-acid2`, `same-suite`.
- The `gbdev` community Discord and the `awesome-gbdev` list.
