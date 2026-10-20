//! Cartridge: ROM, external RAM, and the memory bank controller between them.
//!
//! Each MBC is a `Mapper` that translates CPU addresses into ROM/RAM offsets
//! and absorbs register writes in the 0000-7FFF range. Bank numbers are
//! masked to the actual ROM/RAM size, as the address lines simply aren't
//! connected on smaller carts.

pub trait Mapper {
    /// Physical ROM offset for a CPU read in 0000-7FFF.
    fn rom_offset(&self, addr: u16) -> usize;
    /// Physical RAM offset for a CPU access in A000-BFFF, or None if RAM is
    /// disabled or absent.
    fn ram_offset(&self, addr: u16) -> Option<usize>;
    fn write_register(&mut self, addr: u16, val: u8);
    /// Hook for mappers that intercept RAM-region reads (MBC3 RTC, MBC2 nibbles).
    fn read_ram(&self, ram: &[u8], addr: u16) -> u8 {
        match self.ram_offset(addr) {
            Some(i) if i < ram.len() => ram[i],
            _ => 0xFF,
        }
    }
    fn write_ram(&mut self, ram: &mut [u8], addr: u16, val: u8) {
        if let Some(i) = self.ram_offset(addr) {
            if i < ram.len() {
                ram[i] = val;
            }
        }
    }
    /// Advance mapper-internal clocks by one T-cycle (MBC3 RTC).
    fn tick(&mut self) {}
    /// Extra persistent state beyond RAM (RTC registers), for battery saves.
    fn save_extra(&self) -> Vec<u8> {
        Vec::new()
    }
    fn load_extra(&mut self, _data: &[u8]) {}
    fn box_clone(&self) -> Box<dyn Mapper>;
}

impl Clone for Box<dyn Mapper> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

fn bank_mask(size: usize, bank_size: usize) -> usize {
    (size / bank_size).max(1).next_power_of_two() - 1
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct NoMbc {
    ram_size: usize,
}

impl Mapper for NoMbc {
    fn box_clone(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
    fn rom_offset(&self, addr: u16) -> usize {
        addr as usize
    }
    fn ram_offset(&self, addr: u16) -> Option<usize> {
        let off = addr as usize - 0xA000;
        (off < self.ram_size).then_some(off)
    }
    fn write_register(&mut self, _addr: u16, _val: u8) {}
}

// ---------------------------------------------------------------------------

/// MBC1. `multicart` selects the MBC1M wiring used by multi-game carts: BANK1
/// is only 4 bits wide and BANK2 lands on ROM address bit 18 instead of 19.
#[derive(Clone)]
pub struct Mbc1 {
    ramg: bool,
    bank1: u8,
    bank2: u8,
    mode: bool,
    multicart: bool,
    rom_mask: usize,
    ram_size: usize,
}

impl Mbc1 {
    pub fn new(rom_size: usize, ram_size: usize, multicart: bool) -> Self {
        Mbc1 {
            ramg: false,
            bank1: 1,
            bank2: 0,
            mode: false,
            multicart,
            rom_mask: bank_mask(rom_size, 0x4000),
            ram_size,
        }
    }

    fn high_bank(&self) -> usize {
        if self.multicart {
            ((self.bank2 as usize) << 4) | (self.bank1 as usize & 0x0F)
        } else {
            ((self.bank2 as usize) << 5) | self.bank1 as usize
        }
    }

    fn low_bank(&self) -> usize {
        if !self.mode {
            0
        } else if self.multicart {
            (self.bank2 as usize) << 4
        } else {
            (self.bank2 as usize) << 5
        }
    }
}

impl Mapper for Mbc1 {
    fn box_clone(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
    fn rom_offset(&self, addr: u16) -> usize {
        let bank = if addr < 0x4000 {
            self.low_bank()
        } else {
            self.high_bank()
        };
        ((bank & self.rom_mask) << 14) | (addr as usize & 0x3FFF)
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        if !self.ramg || self.ram_size == 0 {
            return None;
        }
        let bank = if self.mode && self.ram_size > 0x2000 {
            self.bank2 as usize
        } else {
            0
        };
        let off = (bank << 13) | (addr as usize & 0x1FFF);
        Some(off % self.ram_size)
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        match addr >> 13 {
            0 => self.ramg = val & 0x0F == 0x0A,
            1 => {
                let v = val & 0x1F;
                self.bank1 = if v == 0 { 1 } else { v };
            }
            2 => self.bank2 = val & 0x03,
            3 => self.mode = val & 0x01 != 0,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------

/// MBC2: 4-bit ROM bank register, 512 x 4-bit built-in RAM.
#[derive(Clone)]
pub struct Mbc2 {
    ramg: bool,
    romb: u8,
    rom_mask: usize,
}

impl Mbc2 {
    pub fn new(rom_size: usize) -> Self {
        Mbc2 {
            ramg: false,
            romb: 1,
            rom_mask: bank_mask(rom_size, 0x4000),
        }
    }
}

impl Mapper for Mbc2 {
    fn box_clone(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
    fn rom_offset(&self, addr: u16) -> usize {
        let bank = if addr < 0x4000 { 0 } else { self.romb as usize };
        ((bank & self.rom_mask) << 14) | (addr as usize & 0x3FFF)
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        self.ramg.then_some(addr as usize & 0x01FF)
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        if addr >= 0x4000 {
            return;
        }
        if addr & 0x0100 == 0 {
            self.ramg = val & 0x0F == 0x0A;
        } else {
            let v = val & 0x0F;
            self.romb = if v == 0 { 1 } else { v };
        }
    }

    fn read_ram(&self, ram: &[u8], addr: u16) -> u8 {
        match self.ram_offset(addr) {
            Some(i) => 0xF0 | (ram[i] & 0x0F),
            None => 0xFF,
        }
    }

    fn write_ram(&mut self, ram: &mut [u8], addr: u16, val: u8) {
        if let Some(i) = self.ram_offset(addr) {
            ram[i] = val & 0x0F;
        }
    }
}

// ---------------------------------------------------------------------------

/// MBC3 with real-time clock. The RTC is driven by emulated T-cycles rather
/// than the host clock so the core stays deterministic; the frontend can
/// fold in elapsed wall time via `load_extra` when restoring a save.
#[derive(Clone)]
pub struct Mbc3 {
    ramg: bool,
    romb: u8,
    ramb: u8,
    rom_mask: usize,
    ram_size: usize,
    rtc: Rtc,
}

#[derive(Default, Clone, Copy)]
struct RtcRegs {
    s: u8,
    m: u8,
    h: u8,
    dl: u8,
    dh: u8,
}

#[derive(Default, Clone)]
struct Rtc {
    live: RtcRegs,
    latched: RtcRegs,
    latch_armed: bool,
    subsecond: u32,
}

const TCYCLES_PER_SECOND: u32 = 4_194_304;

impl Rtc {
    fn halted(&self) -> bool {
        self.live.dh & 0x40 != 0
    }

    fn tick(&mut self) {
        if self.halted() {
            return;
        }
        self.subsecond += 1;
        if self.subsecond < TCYCLES_PER_SECOND {
            return;
        }
        self.subsecond = 0;
        self.advance_second();
    }

    fn advance_second(&mut self) {
        let r = &mut self.live;
        r.s = (r.s + 1) & 0x3F;
        if r.s != 60 {
            return;
        }
        r.s = 0;
        r.m = (r.m + 1) & 0x3F;
        if r.m != 60 {
            return;
        }
        r.m = 0;
        r.h = (r.h + 1) & 0x1F;
        if r.h != 24 {
            return;
        }
        r.h = 0;
        let days = (((r.dh & 1) as u16) << 8 | r.dl as u16) + 1;
        r.dl = days as u8;
        r.dh = (r.dh & 0xFE) | ((days >> 8) as u8 & 1);
        if days >= 512 {
            r.dh |= 0x80;
        }
    }

    fn read(&self, reg: u8) -> u8 {
        let l = &self.latched;
        match reg {
            0x08 => l.s | 0xC0,
            0x09 => l.m | 0xC0,
            0x0A => l.h | 0xE0,
            0x0B => l.dl,
            0x0C => l.dh | 0x3E,
            _ => 0xFF,
        }
    }

    fn write(&mut self, reg: u8, val: u8) {
        let r = &mut self.live;
        match reg {
            0x08 => {
                r.s = val & 0x3F;
                self.subsecond = 0;
            }
            0x09 => r.m = val & 0x3F,
            0x0A => r.h = val & 0x1F,
            0x0B => r.dl = val,
            0x0C => r.dh = val & 0xC1,
            _ => {}
        }
    }
}

impl Mbc3 {
    pub fn new(rom_size: usize, ram_size: usize) -> Self {
        Mbc3 {
            ramg: false,
            romb: 1,
            ramb: 0,
            rom_mask: bank_mask(rom_size, 0x4000),
            ram_size,
            rtc: Rtc::default(),
        }
    }
}

impl Mapper for Mbc3 {
    fn box_clone(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
    fn rom_offset(&self, addr: u16) -> usize {
        let bank = if addr < 0x4000 { 0 } else { self.romb as usize };
        ((bank & self.rom_mask) << 14) | (addr as usize & 0x3FFF)
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        if !self.ramg || self.ramb > 0x03 || self.ram_size == 0 {
            return None;
        }
        let off = ((self.ramb as usize) << 13) | (addr as usize & 0x1FFF);
        Some(off % self.ram_size)
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        match addr >> 13 {
            0 => self.ramg = val & 0x0F == 0x0A,
            1 => {
                let v = val & 0x7F;
                self.romb = if v == 0 { 1 } else { v };
            }
            2 => self.ramb = val & 0x0F,
            3 => {
                if self.rtc.latch_armed && val == 0x01 {
                    self.rtc.latched = self.rtc.live;
                }
                self.rtc.latch_armed = val == 0x00;
            }
            _ => {}
        }
    }

    fn read_ram(&self, ram: &[u8], addr: u16) -> u8 {
        if !self.ramg {
            return 0xFF;
        }
        if (0x08..=0x0C).contains(&self.ramb) {
            return self.rtc.read(self.ramb);
        }
        match self.ram_offset(addr) {
            Some(i) if i < ram.len() => ram[i],
            _ => 0xFF,
        }
    }

    fn write_ram(&mut self, ram: &mut [u8], addr: u16, val: u8) {
        if !self.ramg {
            return;
        }
        if (0x08..=0x0C).contains(&self.ramb) {
            self.rtc.write(self.ramb, val);
            return;
        }
        if let Some(i) = self.ram_offset(addr) {
            if i < ram.len() {
                ram[i] = val;
            }
        }
    }

    fn tick(&mut self) {
        self.rtc.tick();
    }

    fn save_extra(&self) -> Vec<u8> {
        let l = self.rtc.live;
        let t = self.rtc.latched;
        vec![l.s, l.m, l.h, l.dl, l.dh, t.s, t.m, t.h, t.dl, t.dh]
    }

    fn load_extra(&mut self, d: &[u8]) {
        if d.len() < 10 {
            return;
        }
        self.rtc.live = RtcRegs { s: d[0], m: d[1], h: d[2], dl: d[3], dh: d[4] };
        self.rtc.latched = RtcRegs { s: d[5], m: d[6], h: d[7], dl: d[8], dh: d[9] };
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Mbc5 {
    ramg: bool,
    romb: u16,
    ramb: u8,
    rom_mask: usize,
    ram_size: usize,
}

impl Mbc5 {
    pub fn new(rom_size: usize, ram_size: usize) -> Self {
        Mbc5 {
            ramg: false,
            romb: 1,
            ramb: 0,
            rom_mask: bank_mask(rom_size, 0x4000),
            ram_size,
        }
    }
}

impl Mapper for Mbc5 {
    fn box_clone(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
    fn rom_offset(&self, addr: u16) -> usize {
        let bank = if addr < 0x4000 { 0 } else { self.romb as usize };
        ((bank & self.rom_mask) << 14) | (addr as usize & 0x3FFF)
    }

    fn ram_offset(&self, addr: u16) -> Option<usize> {
        if !self.ramg || self.ram_size == 0 {
            return None;
        }
        let off = ((self.ramb as usize) << 13) | (addr as usize & 0x1FFF);
        Some(off % self.ram_size)
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        match addr >> 12 {
            0 | 1 => self.ramg = val & 0x0F == 0x0A,
            2 => self.romb = (self.romb & 0x100) | val as u16,
            3 => self.romb = (self.romb & 0xFF) | ((val as u16 & 1) << 8),
            4 | 5 => self.ramb = val & 0x0F,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Cartridge {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub mapper: Box<dyn Mapper>,
    pub title: String,
    pub cart_type: u8,
    pub has_battery: bool,
}

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// MBC1 multicarts (MBC1M) aren't flagged in the header. Like mooneye-gb,
/// detect them by the presence of additional Nintendo logos at the starts
/// of the 256 KiB sub-ROMs an 8 Mbit multicart would be divided into.
fn detect_mbc1_multicart(rom: &[u8]) -> bool {
    if rom.len() != 0x100000 {
        return false;
    }
    let logos = (0..4)
        .filter(|&i| {
            let base = i * 0x40000 + 0x104;
            rom.get(base..base + 48) == Some(&NINTENDO_LOGO[..])
        })
        .count();
    logos >= 2
}

impl Cartridge {
    pub fn from_rom(rom: &[u8]) -> Self {
        let mut data = rom.to_vec();
        if data.len() < 0x8000 {
            data.resize(0x8000, 0xFF);
        }
        let cart_type = data[0x147];
        let mut ram_size = match data[0x149] {
            0x01 => 2 * 1024,
            0x02 => 8 * 1024,
            0x03 => 32 * 1024,
            0x04 => 128 * 1024,
            0x05 => 64 * 1024,
            _ => 0,
        };
        if matches!(cart_type, 0x05 | 0x06) {
            ram_size = 512;
        }
        let has_battery = matches!(
            cart_type,
            0x03 | 0x06 | 0x09 | 0x0D | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E | 0x22 | 0xFF
        );
        let rom_size = data.len();
        let mapper: Box<dyn Mapper> = match cart_type {
            0x00 | 0x08 | 0x09 => Box::new(NoMbc { ram_size }),
            0x01..=0x03 => Box::new(Mbc1::new(
                rom_size,
                ram_size,
                detect_mbc1_multicart(&data),
            )),
            0x05 | 0x06 => Box::new(Mbc2::new(rom_size)),
            0x0F..=0x13 => Box::new(Mbc3::new(rom_size, ram_size)),
            0x19..=0x1E => Box::new(Mbc5::new(rom_size, ram_size)),
            t => panic!("unsupported cartridge type {t:#04x}"),
        };
        let title_bytes: Vec<u8> = data[0x134..0x144]
            .iter()
            .take_while(|&&b| b != 0)
            .copied()
            .collect();
        let title = String::from_utf8_lossy(&title_bytes).to_string();
        Cartridge {
            rom: data,
            ram: vec![0; ram_size],
            mapper,
            title,
            cart_type,
            has_battery,
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        self.rom
            .get(self.mapper.rom_offset(addr))
            .copied()
            .unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, addr: u16, val: u8) {
        self.mapper.write_register(addr, val);
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        self.mapper.read_ram(&self.ram, addr)
    }

    pub fn write_ram(&mut self, addr: u16, val: u8) {
        self.mapper.write_ram(&mut self.ram, addr, val);
    }

    pub fn tick(&mut self) {
        self.mapper.tick();
    }

    /// Battery-backed state: external RAM followed by mapper extras (RTC).
    /// Empty if the cart has no battery.
    pub fn save_data(&self) -> Vec<u8> {
        if !self.has_battery {
            return Vec::new();
        }
        let mut out = self.ram.clone();
        out.extend(self.mapper.save_extra());
        out
    }

    pub fn load_save_data(&mut self, data: &[u8]) {
        let n = self.ram.len().min(data.len());
        self.ram[..n].copy_from_slice(&data[..n]);
        if data.len() > self.ram.len() {
            self.mapper.load_extra(&data[self.ram.len()..]);
        }
    }
}
