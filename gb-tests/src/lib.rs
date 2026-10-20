use gb_core::GameBoy;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
        }
    }
}

/// How a ROM reports its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Oracle {
    /// blargg: "Passed"/"Failed" over serial, or the cart-RAM signature
    /// (DE B0 61 at A001, exit code at A000) used by the newer shell.
    Blargg,
    /// mooneye: executes `LD B,B`; Fibonacci registers mean pass.
    Mooneye,
    /// acid2-style: render a fixed number of frames, hash the framebuffer and
    /// compare against the committed hash of the reference PNG.
    Framebuffer { expected: u64, frames: u64 },
}

/// FNV-1a 64 of `reference-dmg.png` decoded to 160x144 bytes in {FF,AA,55,00}.
pub const DMG_ACID2_HASH: u64 = 0xbaa9_eb2e_a362_d6fe;

impl Oracle {
    pub fn for_rom(rom: &str) -> Oracle {
        if rom.starts_with("mooneye/") {
            Oracle::Mooneye
        } else if rom == "dmg-acid2/dmg-acid2.gb" {
            Oracle::Framebuffer {
                expected: DMG_ACID2_HASH,
                frames: 12,
            }
        } else {
            Oracle::Blargg
        }
    }

    pub fn budget_mcycles(self) -> u64 {
        match self {
            Oracle::Blargg => 80_000_000,
            Oracle::Mooneye => 8_000_000,
            Oracle::Framebuffer { .. } => 30_000_000,
        }
    }
}

pub struct RomResult {
    pub status: Status,
    pub detail: String,
    pub mcycles: u64,
}

pub fn roms_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("roms")
}

pub fn ratchet_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ratchet.txt")
}

pub fn run_rom(rom: &str) -> RomResult {
    let oracle = Oracle::for_rom(rom);
    let data = std::fs::read(roms_dir().join(rom)).unwrap_or_else(|e| panic!("{rom}: {e}"));
    let mut gb = GameBoy::new(&data);
    match oracle {
        Oracle::Blargg => run_blargg(&mut gb, oracle.budget_mcycles()),
        Oracle::Mooneye => run_mooneye(&mut gb, oracle.budget_mcycles()),
        Oracle::Framebuffer { expected, frames } => {
            run_framebuffer(&mut gb, expected, frames, oracle.budget_mcycles())
        }
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn run_framebuffer(gb: &mut GameBoy, expected: u64, frames: u64, budget: u64) -> RomResult {
    let mut cycles = 0;
    while gb.bus.ppu.frame_count < frames && cycles < budget {
        cycles += gb.step() as u64;
    }
    let hash = fnv1a64(gb.framebuffer());
    RomResult {
        status: if hash == expected { Status::Pass } else { Status::Fail },
        detail: format!("framebuffer fnv1a64={hash:#018x} expected={expected:#018x}"),
        mcycles: cycles,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn has_cart_signature(gb: &GameBoy) -> bool {
    gb.bus.peek8(0xA001) == 0xDE && gb.bus.peek8(0xA002) == 0xB0 && gb.bus.peek8(0xA003) == 0x61
}

fn run_blargg(gb: &mut GameBoy, budget: u64) -> RomResult {
    let mut status = None;
    let mut cycles = 0;
    // The cart-RAM shell writes the DE B0 61 signature, then $80 to A000 to
    // mean "running", then the real exit code. Only trust A000 once we have
    // seen the $80, otherwise the RAM's power-on contents decide the result.
    let mut armed = false;
    while cycles < budget {
        cycles += gb.step() as u64;
        let out = gb.serial_output();
        if contains(out, b"Passed") {
            status = Some(Status::Pass);
            break;
        }
        if contains(out, b"Failed") {
            status = Some(Status::Fail);
            break;
        }
        if has_cart_signature(gb) {
            let code = gb.bus.peek8(0xA000);
            if code == 0x80 {
                armed = true;
            } else if armed {
                status = Some(if code == 0 { Status::Pass } else { Status::Fail });
                break;
            }
        }
        // Older blargg shells print to the LCD only; once the program parks
        // in its post-exit `jr -2` loop, read the result off the tilemap.
        let pc = gb.cpu.pc;
        if gb.bus.peek8(pc) == 0x18 && gb.bus.peek8(pc.wrapping_add(1)) == 0xFE {
            let bg_map = &gb.bus.ppu.vram[0x1800..0x1C00];
            if contains(bg_map, b"Passed") {
                status = Some(Status::Pass);
                break;
            }
            if contains(bg_map, b"Failed") {
                status = Some(Status::Fail);
                break;
            }
        }
    }
    let mut detail = String::from_utf8_lossy(gb.serial_output()).into_owned();
    if has_cart_signature(gb) {
        let text: Vec<u8> = (0..0x1000u16)
            .map(|i| gb.bus.peek8(0xA004 + i))
            .take_while(|&b| b != 0)
            .collect();
        detail += &format!(
            "\n[cart-ram exit={:#04x}] {}",
            gb.bus.peek8(0xA000),
            String::from_utf8_lossy(&text)
        );
    }
    if status.is_none() {
        detail += &format!("\n[timeout after {cycles} M-cycles]");
    }
    RomResult {
        status: status.unwrap_or(Status::Fail),
        detail,
        mcycles: cycles,
    }
}

fn run_mooneye(gb: &mut GameBoy, budget: u64) -> RomResult {
    let mut cycles = 0;
    while cycles < budget {
        let pc = gb.cpu.pc;
        let is_ld_b_b = !gb.cpu.halted && gb.bus.peek8(pc) == 0x40;
        cycles += gb.step() as u64;
        if is_ld_b_b && gb.cpu.pc == pc.wrapping_add(1) {
            let c = &gb.cpu;
            let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
            let status = if regs == [3, 5, 8, 13, 21, 34] {
                Status::Pass
            } else {
                Status::Fail
            };
            return RomResult {
                status,
                detail: format!(
                    "LD B,B at {pc:#06x}: B={:#04x} C={:#04x} D={:#04x} E={:#04x} H={:#04x} L={:#04x}",
                    regs[0], regs[1], regs[2], regs[3], regs[4], regs[5]
                ),
                mcycles: cycles,
            };
        }
    }
    RomResult {
        status: Status::Fail,
        detail: format!("[timeout after {cycles} M-cycles, pc={:#06x}]", gb.cpu.pc),
        mcycles: cycles,
    }
}

pub struct RatchetEntry {
    pub rom: String,
    pub expected: Status,
}

pub fn parse_ratchet(text: &str) -> Vec<RatchetEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (status, rom) = l
                .split_once(' ')
                .unwrap_or_else(|| panic!("bad ratchet line: {l}"));
            let expected = match status {
                "pass" => Status::Pass,
                "fail" => Status::Fail,
                other => panic!("bad ratchet status {other}"),
            };
            RatchetEntry {
                rom: rom.trim().to_string(),
                expected,
            }
        })
        .collect()
}

/// Every `.gb` file under `roms/`, as a path relative to it.
pub fn tracked_roms() -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "gb") {
                let rel = path.strip_prefix(root).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let root = roms_dir();
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

pub fn load_ratchet() -> Vec<RatchetEntry> {
    let text = std::fs::read_to_string(ratchet_path()).expect("ratchet.txt missing");
    parse_ratchet(&text)
}

pub fn expected_status(rom: &str) -> Status {
    load_ratchet()
        .into_iter()
        .find(|e| e.rom == rom)
        .unwrap_or_else(|| panic!("{rom} not in ratchet.txt"))
        .expected
}
