use gb_core::GameBoy;
use std::path::{Path, PathBuf};

pub const MAX_MCYCLES: u64 = 30_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
}

pub struct RomResult {
    pub status: Status,
    pub serial: String,
    pub mcycles: u64,
}

pub fn roms_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("roms")
}

pub fn run_serial_rom(path: &Path, max_mcycles: u64) -> RomResult {
    let rom = std::fs::read(path).expect("read rom");
    let mut gb = GameBoy::new(&rom);
    let mut mcycles = 0u64;
    let mut status = None;
    while mcycles < max_mcycles {
        gb.step();
        mcycles += 1;
        let out = gb.serial_output();
        if windows_contains(out, b"Passed") {
            status = Some(Status::Pass);
            break;
        }
        if windows_contains(out, b"Failed") {
            status = Some(Status::Fail);
            break;
        }
        // BGB-style memory signature: DE B0 61 at A001-A003, result at A000
        if gb.bus.peek8(0xA001) == 0xDE
            && gb.bus.peek8(0xA002) == 0xB0
            && gb.bus.peek8(0xA003) == 0x61
        {
            let result = gb.bus.peek8(0xA000);
            if result != 0x80 {
                status = Some(if result == 0 { Status::Pass } else { Status::Fail });
                break;
            }
        }
    }
    let mut serial = String::from_utf8_lossy(gb.serial_output()).to_string();
    if gb.bus.peek8(0xA001) == 0xDE && gb.bus.peek8(0xA002) == 0xB0 && gb.bus.peek8(0xA003) == 0x61
    {
        let mut text = Vec::new();
        for i in 0..1024u16 {
            let b = gb.bus.peek8(0xA004 + i);
            if b == 0 {
                break;
            }
            text.push(b);
        }
        serial += &format!(
            "\n[cart-ram result={:#04x}] {}",
            gb.bus.peek8(0xA000),
            String::from_utf8_lossy(&text)
        );
    }
    RomResult {
        status: status.unwrap_or(Status::Fail),
        serial,
        mcycles,
    }
}

fn windows_contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle)
}

pub struct RatchetEntry {
    pub rom: String,
    pub expected: Status,
}

pub fn load_ratchet() -> Vec<RatchetEntry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ratchet.txt");
    let text = std::fs::read_to_string(&path).expect("ratchet.txt missing");
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (status, rom) = l.trim().split_once(' ').expect("ratchet line needs status and rom");
            let status = match status {
                "pass" => Status::Pass,
                "fail" => Status::Fail,
                other => panic!("bad ratchet status {other}"),
            };
            RatchetEntry {
                rom: rom.trim().to_string(),
                expected: status,
            }
        })
        .collect()
}
