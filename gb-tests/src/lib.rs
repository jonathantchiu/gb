use gb_core::GameBoy;
use std::path::{Path, PathBuf};

/// Recorded/actual outcome of a ROM in the ratchet.
///
/// - `Pass` / `Fail`: the emulation result.
/// - `NotApplicable`: the ROM targets a different hardware revision or
///   needs the real boot ROM; skipped, and excluded from the denominator.
/// - `Timeout`: the ROM never reported a result within its cycle budget.
///   Distinct from `Fail` because the causes (and fixes) differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Pass,
    Fail,
    NotApplicable,
    Timeout,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::NotApplicable => "not-applicable",
            Status::Timeout => "timeout",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s.trim() {
            "pass" => Some(Status::Pass),
            "fail" => Some(Status::Fail),
            "not-applicable" => Some(Status::NotApplicable),
            "timeout" => Some(Status::Timeout),
            _ => None,
        }
    }

    /// Ordinates over which a timing budget applies (all but N/A).
    pub fn runs_emulator(self) -> bool {
        !matches!(self, Status::NotApplicable)
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
    /// acid2-style: render fixed frames, then compare the framebuffer against
    /// the decoded upstream reference image, pixel-for-pixel.
    Framebuffer {
        frames: u64,
        reference_png: &'static str,
    },
}

/// The canonical framebuffer is RGBA8888; DMG shades map to greys
/// FF/AA/55/00. This decoder exists so the oracle compares against the
/// upstream reference image, never against a hash of our own output.
mod reference_png {
    pub fn decode_rgba(png: &[u8]) -> Option<Vec<u8>> {
        let mut pos = 8usize;
        if png.get(0..8) != Some(b"\x89PNG\r\n\x1a\n") {
            return None;
        }
        let mut idat = Vec::new();
        let (mut w, mut h, mut bit_depth, mut color_type, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
        let mut plte: Vec<u8> = Vec::new();
        let mut trns: Vec<u8> = Vec::new();
        while pos + 8 <= png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().ok()?) as usize;
            let ty = &png[pos + 4..pos + 8];
            let data = png.get(pos + 8..pos + 8 + len)?;
            match ty {
                b"IHDR" => {
                    w = u32::from_be_bytes(data[0..4].try_into().ok()?);
                    h = u32::from_be_bytes(data[4..8].try_into().ok()?);
                    bit_depth = data[8];
                    color_type = data[9];
                    interlace = data[12];
                }
                b"PLTE" => plte = data.to_vec(),
                b"tRNS" => trns = data.to_vec(),
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => break,
                _ => {}
            }
            pos += 12 + len;
        }
        if interlace != 0 || w == 0 || h == 0 {
            return None;
        }
        let raw = miniz_decompress(&idat)?;
        let channels: usize = match color_type {
            0 => 1,
            2 => 3,
            3 => 1,
            4 => 2,
            6 => 4,
            _ => return None,
        };
        let bits_per_pixel = channels * bit_depth as usize;
        let stride = (w as usize * bits_per_pixel).div_ceil(8);
        if raw.len() < h as usize * (stride + 1) {
            return None;
        }
        // Unfilter.
        let mut px = vec![0u8; h as usize * stride];
        for y in 0..h as usize {
            let s = y * (stride + 1);
            let ft = raw[s];
            let row = &raw[s + 1..s + 1 + stride];
            for x in 0..stride {
                let a = if x >= bits_per_pixel / 8 { px[y * stride + x - bits_per_pixel / 8] } else { 0 };
                let b = if y > 0 { px[(y - 1) * stride + x] } else { 0 };
                let c = if y > 0 && x >= bits_per_pixel / 8 { px[(y - 1) * stride + x - bits_per_pixel / 8] } else { 0 };
                let pred = match ft {
                    0 => 0u16,
                    1 => a as u16,
                    2 => b as u16,
                    3 => (a as u16 + b as u16) / 2,
                    4 => {
                        let (a, b, c) = (a as i32, b as i32, c as i32);
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        let pr = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
                        pr as u16
                    }
                    _ => return None,
                };
                px[y * stride + x] = row[x].wrapping_add(pred as u8);
            }
        }
        // Expand to RGBA8888.
        let mut out = vec![0u8; w as usize * h as usize * 4];
        let get_px = |idx: usize| -> (u8, u8, u8, u8) {
            let x = idx % w as usize;
            let y = idx / w as usize;
            let base = y * stride;
            match (color_type, bit_depth) {
                (0, 8) => {
                    let g = px[base + x];
                    (g, g, g, 255)
                }
                (0, 2) => {
                    let byte = px[base + x / 4];
                    let shift = 6 - 2 * (x % 4);
                    let g2 = (byte >> shift) & 3;
                    let g = [0, 85, 170, 255][g2 as usize];
                    (g, g, g, 255)
                }
                (0, 1) | (0, 4) | (0, 16) => {
                    // Fallback: sample high bits coarsely.
                    let byte = px[base + (x * bit_depth as usize) / 8];
                    let g = byte;
                    (g, g, g, 255)
                }
                (2, 8) => {
                    let o = base + x * 3;
                    (px[o], px[o + 1], px[o + 2], 255)
                }
                (3, 8) => {
                    let pi = px[base + x] as usize;
                    let o = pi * 3;
                    let a = trns.get(pi).copied().unwrap_or(255);
                    (plte[o], plte[o + 1], plte[o + 2], a)
                }
                (4, 8) => {
                    let o = base + x * 2;
                    let g = px[o];
                    (g, g, g, px[o + 1])
                }
                (6, 8) => {
                    let o = base + x * 4;
                    (px[o], px[o + 1], px[o + 2], px[o + 3])
                }
                _ => (0, 0, 0, 255),
            }
        };
        for i in 0..(w as usize * h as usize) {
            let (r, g, b, a) = get_px(i);
            out[i * 4] = r;
            out[i * 4 + 1] = g;
            out[i * 4 + 2] = b;
            out[i * 4 + 3] = a;
        }
        Some(out)
    }

    /// Minimal zlib/DEFLATE (stored blocks only if uncompressed; otherwise
    /// a small inflate). Reference PNGs from the acid2 repo use dynamic
    /// Huffman; implement full inflate.
    pub fn miniz_decompress(zlib: &[u8]) -> Option<Vec<u8>> {
        if zlib.len() < 2 {
            return None;
        }
        let mut bit = BitReader::new(&zlib[2..]);
        let mut out = Vec::new();
        loop {
            let bfinal = bit.bits(1)?;
            let btype = bit.bits(2)?;
            match btype {
                0 => {
                    bit.align_byte();
                    let len = bit.bits_raw_u16()?;
                    let data = bit.take_bytes(len);
                    out.extend_from_slice(data?);
                }
                1 | 2 => {
                    let (lit, dist) = if btype == 1 {
                        let lit_lens: Vec<u8> = (0..288)
                            .map(|i| if i < 144 { 8 } else if i < 256 { 9 } else if i < 280 { 7 } else { 8 })
                            .collect();
                        (Huffman::new(&lit_lens)?, Huffman::new(&[5u8; 32])?)
                    } else {
                        let hlit = bit.bits(5)? as usize + 257;
                        let hdist = bit.bits(5)? as usize + 1;
                        let hclen = bit.bits(4)? as usize + 4;
                        const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                        let mut code_lens = [0u8; 19];
                        for &idx in ORDER.iter().take(hclen) {
                            code_lens[idx] = bit.bits(3)? as u8;
                        }
                        let code_huff = Huffman::new(&code_lens)?;
                        let total = hlit + hdist;
                        let mut lens = vec![0u8; total];
                        let mut i = 0;
                        while i < total {
                            let sym = code_huff.decode(&mut bit)?;
                            match sym {
                                0..=15 => {
                                    lens[i] = sym as u8;
                                    i += 1;
                                }
                                16 => {
                                    if i == 0 {
                                        return None;
                                    }
                                    let rep = 3 + bit.bits(2)? as usize;
                                    let prev = lens[i - 1];
                                    for _ in 0..rep {
                                        if i < total {
                                            lens[i] = prev;
                                            i += 1;
                                        }
                                    }
                                }
                                17 => {
                                    let rep = 3 + bit.bits(3)? as usize;
                                    i += rep;
                                }
                                18 => {
                                    let rep = 11 + bit.bits(7)? as usize;
                                    i += rep;
                                }
                                _ => return None,
                            }
                        }
                        if lens[256] == 0 {
                            return None;
                        }
                        let lit_lens = lens[..hlit].to_vec();
                        let dist_lens = lens[hlit..].to_vec();
                        (Huffman::new(&lit_lens)?, Huffman::new(&dist_lens)?)
                    };
                    loop {
                        let sym = lit.decode(&mut bit)?;
                        match sym {
                            0..=255 => out.push(sym as u8),
                            256 => break,
                            257..=285 => {
                                const LEN_BASE: [usize; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
                                const LEN_EXT: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
                                const DIST_BASE: [usize; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
                                const DIST_EXT: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
                                let li = (sym - 257) as usize;
                                let len = LEN_BASE[li] + bit.bits(LEN_EXT[li])? as usize;
                                let dsym = dist.decode(&mut bit)? as usize;
                                let d = DIST_BASE[dsym] + bit.bits(DIST_EXT[dsym])? as usize;
                                if d > out.len() {
                                    return None;
                                }
                                let start = out.len() - d;
                                for k in 0..len {
                                    let b = out[start + k];
                                    out.push(b);
                                }
                            }
                            _ => return None,
                        }
                    }
                }
                _ => return None,
            }
            if bfinal == 1 {
                break;
            }
        }
        Some(out)
    }

    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
        bitbuf: u64,
        bitcnt: u32,
    }
    impl<'a> BitReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            BitReader { data, pos: 0, bitbuf: 0, bitcnt: 0 }
        }
        fn bits(&mut self, n: u32) -> Option<u32> {
            while self.bitcnt < n {
                let byte = *self.data.get(self.pos)? as u64;
                self.pos += 1;
                self.bitbuf |= byte << self.bitcnt;
                self.bitcnt += 8;
            }
            if n == 0 { return Some(0); }
            let v = self.bitbuf & ((1u64 << n) - 1);
            self.bitbuf >>= n;
            self.bitcnt -= n;
            Some(v as u32)
        }
        fn align_byte(&mut self) {
            self.bitbuf = 0;
            self.bitcnt = 0;
        }
        fn bits_raw_u16(&mut self) -> Option<usize> {
            let lo = *self.data.get(self.pos)? as usize;
            let hi = *self.data.get(self.pos + 1)? as usize;
            let len = lo | (hi << 8);
            Some(len)
        }
        fn take_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
            let s = self.data.get(self.pos..self.pos + n)?;
            self.pos += n;
            Some(s)
        }
    }

    struct Huffman {
        // (code, len, symbol) sorted for canonical decoding via counts.
        counts: [u16; 16],
        symbols: Vec<u16>,
    }
    impl Huffman {
        fn new(lengths: &[u8]) -> Option<Self> {
            let mut counts = [0u16; 16];
            for &l in lengths {
                if l > 15 {
                    return None;
                }
                counts[l as usize] += 1;
            }
            counts[0] = 0;
            // Reject over-subscribed code tables (Kraft sum > 1).
            let mut kraft: i64 = 0;
            for len in 1..16 {
                kraft += (counts[len] as i64) << (15 - len);
            }
            if kraft > (1i64 << 15) {
                return None;
            }
            let mut offs = [0u16; 16];
            for i in 1..16 {
                offs[i] = offs[i - 1] + counts[i - 1];
            }
            let mut symbols = vec![0u16; lengths.iter().filter(|&&l| l != 0).count()];
            for (sym, &l) in lengths.iter().enumerate() {
                if l != 0 {
                    symbols[offs[l as usize] as usize] = sym as u16;
                    offs[l as usize] += 1;
                }
            }
            Some(Huffman { counts, symbols })
        }
        fn decode(&self, bit: &mut BitReader) -> Option<u16> {
            // Canonical Huffman decoding (RFC 1951 puff style).
            let mut code: i32 = 0;
            let mut first: i32 = 0;
            let mut index: i32 = 0;
            for len in 1..16 {
                code |= bit.bits(1)? as i32;
                let count = self.counts[len] as i32;
                if code - count < first {
                    return Some(self.symbols[(index + (code - first)) as usize]);
                }
                index += count;
                first += count;
                first <<= 1;
                code <<= 1;
            }
            None
        }
    }
}


pub fn debug_inflate_len(z: &[u8]) -> Option<usize> { reference_png::miniz_decompress(z).map(|v| v.len()) }

impl Oracle {
    pub fn for_rom(rom: &str) -> Oracle {
        if rom.starts_with("mooneye/") || rom.starts_with("age/") {
            Oracle::Mooneye
        } else if rom == "dmg-acid2/dmg-acid2.gb" {
            Oracle::Framebuffer {
                frames: 12,
                reference_png: "dmg-acid2/reference-dmg.png",
            }
        } else if rom == "cgb-acid2/cgb-acid2.gbc" {
            Oracle::Framebuffer {
                frames: 12,
                reference_png: "cgb-acid2/reference.png",
            }
        } else {
            Oracle::Blargg
        }
    }
}

pub struct RomResult {
    pub status: Status,
    pub detail: String,
    pub mcycles: u64,
    /// Determinism fingerprint: FNV-1a over final framebuffer + CPU registers.
    /// Not an oracle; printed for cross-run comparison.
    pub hash: u64,
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn final_hash(gb: &GameBoy) -> u64 {
    let mut buf = Vec::with_capacity(gb.framebuffer().len() + 16);
    buf.extend_from_slice(gb.framebuffer());
    let c = &gb.cpu;
    buf.extend_from_slice(&[c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l]);
    buf.extend_from_slice(&c.sp.to_be_bytes());
    buf.extend_from_slice(&c.pc.to_be_bytes());
    fnv1a64(&buf)
}

pub fn roms_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("roms")
}

pub fn ratchet_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ratchet.txt")
}

/// Per-ROM emulated-cycle budget (M-cycles). Deterministic: expressed in
/// emulated cycles, not wall time.
pub fn budget_for(entry: &RatchetEntry) -> u64 {
    if let Some(t) = entry.timeout_mcycles {
        return t;
    }
    match Oracle::for_rom(&entry.rom) {
        Oracle::Blargg => 80_000_000,
        Oracle::Mooneye => 8_000_000,
        Oracle::Framebuffer { .. } => 30_000_000,
    }
}

pub fn run_rom(rom: &str, budget_mcycles: u64) -> RomResult {
    let oracle = Oracle::for_rom(rom);
    let data = match std::fs::read(roms_dir().join(rom)) {
        Ok(d) => d,
        Err(_) => {
            return RomResult {
                status: Status::NotApplicable,
                detail: format!("{rom}: file not found"),
                mcycles: 0,
                hash: 0,
            }
        }
    };
    let mut gb = GameBoy::new(&data);
    let mut r = match oracle {
        Oracle::Blargg => run_blargg(&mut gb, budget_mcycles),
        Oracle::Mooneye => run_mooneye(&mut gb, budget_mcycles),
        Oracle::Framebuffer { frames, reference_png } => {
            run_framebuffer(&mut gb, frames, reference_png, budget_mcycles)
        }
    };
    r.hash = final_hash(&gb);
    r
}

fn run_framebuffer(gb: &mut GameBoy, frames: u64, reference_png: &str, budget: u64) -> RomResult {
    let mut cycles = 0u64;
    while gb.bus.ppu.frame_count < frames && cycles < budget {
        cycles += gb.step() as u64;
    }
    if gb.bus.ppu.frame_count < frames {
        return RomResult {
            status: Status::Timeout,
            detail: format!("only {} frames in {budget} M-cycles", gb.bus.ppu.frame_count),
            mcycles: cycles,
            hash: 0,
        };
    }
    let png = std::fs::read(roms_dir().join(reference_png)).expect("reference png");
    let expected = reference_png::decode_rgba(&png).expect("decode reference png");
    let fb = gb.framebuffer();
    // Canonical framebuffer is DMG greys {FF,AA,55,00} in a single byte;
    // expand to RGBA8888 for comparison.
    let mut actual = vec![0u8; fb.len() * 4];
    for (i, &g) in fb.iter().enumerate() {
        actual[i * 4] = g;
        actual[i * 4 + 1] = g;
        actual[i * 4 + 2] = g;
        actual[i * 4 + 3] = 255;
    }
    if actual == expected {
        RomResult { status: Status::Pass, detail: "framebuffer matches reference PNG".into(), mcycles: cycles, hash: 0 }
    } else {
        let diffs = actual.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a != b).count();
        RomResult {
            status: Status::Fail,
            detail: format!("framebuffer differs from reference PNG in {diffs} pixels"),
            mcycles: cycles,
            hash: 0,
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn has_cart_signature(gb: &GameBoy) -> bool {
    gb.bus.peek8(0xA001) == 0xDE && gb.bus.peek8(0xA002) == 0xB0 && gb.bus.peek8(0xA003) == 0x61
}

fn run_blargg(gb: &mut GameBoy, budget: u64) -> RomResult {
    let mut status: Option<Status> = None;
    let mut cycles = 0u64;
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
    match status {
        Some(s) => RomResult { status: s, detail, mcycles: cycles, hash: 0 },
        None => RomResult {
            status: Status::Timeout,
            detail: format!("{detail}\n[timeout after {cycles} M-cycles]"),
            mcycles: cycles, hash: 0,
        },
    }
}

fn run_mooneye(gb: &mut GameBoy, budget: u64) -> RomResult {
    let mut cycles = 0u64;
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
                mcycles: cycles, hash: 0,
            };
        }
    }
    RomResult {
        status: Status::Timeout,
        detail: format!("[timeout after {cycles} M-cycles, pc={:#06x}]", gb.cpu.pc),
        mcycles: cycles, hash: 0,
    }
}

#[derive(Debug, Clone)]
pub struct RatchetEntry {
    pub rom: String,
    pub expected: Status,
    pub reason: Option<String>,
    /// Per-ROM emulated cycle budget (M-cycles).
    pub timeout_mcycles: Option<u64>,
}

pub fn parse_ratchet(text: &str) -> Vec<RatchetEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            // Format: <status> <rom> [key=value ...] [; reason]
            let (head, reason) = match l.split_once(';') {
                Some((h, r)) => (h.trim(), Some(r.trim().to_string()).filter(|s| !s.is_empty())),
                None => (l, None),
            };
            let (status_s, rest) = head
                .split_once(char::is_whitespace)
                .unwrap_or_else(|| panic!("bad ratchet line: {l}"));
            let expected = Status::parse(status_s)
                .unwrap_or_else(|| panic!("bad ratchet status {status_s} in line: {l}"));
            // Rom paths may contain spaces; `timeout=` marks the end of the path.
            let (rom, timeout_mcycles) = match rest.rsplit_once(" timeout=") {
                Some((r, t)) => (
                    r.trim().to_string(),
                    Some(
                        t.trim()
                            .parse::<u64>()
                            .unwrap_or_else(|_| panic!("bad timeout in line: {l}")),
                    ),
                ),
                None => (rest.trim().to_string(), None),
            };
            RatchetEntry { rom, expected, reason, timeout_mcycles }
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
            } else if path.extension().is_some_and(|e| e == "gb" || e == "gbc") {
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

pub fn ratchet_entry(rom: &str) -> RatchetEntry {
    load_ratchet()
        .into_iter()
        .find(|e| e.rom == rom)
        .unwrap_or_else(|| panic!("{rom} not in ratchet.txt"))
}
