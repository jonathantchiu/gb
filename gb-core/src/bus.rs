use crate::cartridge::Cartridge;
use crate::dma::OamDma;
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

/// Owns every component and mediates all CPU memory access.
///
/// Each CPU bus access is one M-cycle, structured as:
/// 1. `begin_mcycle`: M-cycle-granular state (timer cycle-B window, OAM DMA byte)
/// 2. the CPU's read or write, if any
/// 3. four T-cycles of timer, serial and PPU
pub struct Bus {
    pub cart: Cartridge,
    pub ppu: Ppu,
    pub timer: Timer,
    pub serial: Serial,
    pub joypad: Joypad,
    pub dma: OamDma,
    pub wram: [u8; 0x2000],
    pub hram: [u8; 0x7F],
    pub ie: u8,
    pub if_reg: u8,
    pub mcycles: u64,
}

/// Which physical bus an address is decoded on. While OAM DMA is copying from
/// one of these, CPU reads from the same bus see the DMA's byte instead.
#[derive(PartialEq, Eq)]
enum PhysBus {
    External,
    Vram,
    Internal,
}

fn phys_bus(addr: u16) -> PhysBus {
    match addr {
        0x8000..=0x9FFF => PhysBus::Vram,
        0xFE00..=0xFFFF => PhysBus::Internal,
        _ => PhysBus::External,
    }
}

/// Internal DIV counter value when the DMG ABC boot ROM hands off at 0x0100.
const POST_BOOT_DIV_COUNTER: u16 = 0xABCC;

impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        Bus {
            cart,
            ppu: Ppu::new(),
            timer: Timer::new().with_div_counter(POST_BOOT_DIV_COUNTER),
            serial: Serial::new(),
            joypad: Joypad::new(),
            dma: OamDma::new(),
            wram: [0; 0x2000],
            hram: [0; 0x7F],
            ie: 0,
            if_reg: 0xE1,
            mcycles: 0,
        }
    }

    fn begin_mcycle(&mut self) {
        self.mcycles += 1;
        self.timer.begin_mcycle();
        if let Some(t) = self.dma.begin_mcycle() {
            let b = self.dma_source_read(t.src);
            self.ppu.oam[t.dst as usize] = b;
        }
    }

    fn end_mcycle(&mut self) {
        for _ in 0..4 {
            let old_div = self.timer.div_counter();
            self.timer.tick(&mut self.if_reg);
            self.serial
                .tick(old_div, self.timer.div_counter(), &mut self.if_reg);
            self.ppu.tick(&mut self.if_reg);
        }
    }

    /// An M-cycle with no CPU bus access.
    pub fn tick_mcycle(&mut self) {
        self.begin_mcycle();
        self.end_mcycle();
    }

    pub fn read8(&mut self, addr: u16) -> u8 {
        self.begin_mcycle();
        let v = self.cpu_read(addr);
        self.end_mcycle();
        v
    }

    pub fn write8(&mut self, addr: u16, val: u8) {
        self.begin_mcycle();
        self.cpu_write(addr, val);
        self.end_mcycle();
    }

    fn dma_conflict(&self, addr: u16) -> Option<u8> {
        let t = self.dma.current()?;
        let bus = phys_bus(addr);
        if bus == PhysBus::Internal {
            return (0xFE00..=0xFEFF).contains(&addr).then_some(0xFF);
        }
        (bus == phys_bus(t.src)).then(|| self.dma_source_read(t.src))
    }

    fn cpu_read(&self, addr: u16) -> u8 {
        if let Some(v) = self.dma_conflict(addr) {
            return v;
        }
        self.peek8(addr)
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if self.dma_conflict(addr).is_some() {
            return;
        }
        self.poke8(addr, val);
    }

    /// What OAM DMA sees at `addr`. Sources at E000 and above are decoded as
    /// the external bus, which mirrors WRAM there.
    fn dma_source_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[addr as usize - 0x8000],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            _ => self.wram[(addr as usize - 0xC000) & 0x1FFF],
        }
    }

    /// Side-effect-free view of memory, ignoring DMA. For debuggers and tests.
    pub fn peek8(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[addr as usize - 0x8000],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[addr as usize - 0xC000],
            0xE000..=0xFDFF => self.wram[addr as usize - 0xE000],
            0xFE00..=0xFE9F => self.ppu.oam[addr as usize - 0xFE00],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00 => self.joypad.read(),
            0xFF01 => self.serial.sb,
            0xFF02 => self.serial.read_sc(),
            0xFF04 => self.timer.div(),
            0xFF05 => self.timer.tima,
            0xFF06 => self.timer.tma,
            0xFF07 => self.timer.tac | 0xF8,
            0xFF0F => self.if_reg | 0xE0,
            0xFF40 => self.ppu.read_lcdc(),
            0xFF41 => self.ppu.read_stat(),
            0xFF42 => self.ppu.scy,
            0xFF43 => self.ppu.scx,
            0xFF44 => self.ppu.ly,
            0xFF45 => self.ppu.lyc,
            0xFF46 => self.dma.reg,
            0xFF47 => self.ppu.bgp,
            0xFF48 => self.ppu.obp0,
            0xFF49 => self.ppu.obp1,
            0xFF4A => self.ppu.wy,
            0xFF4B => self.ppu.wx,
            0xFF80..=0xFFFE => self.hram[addr as usize - 0xFF80],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    /// Raw write with register side effects, ignoring DMA. CPU writes go
    /// through `write8`.
    pub fn poke8(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, val),
            0x8000..=0x9FFF => self.ppu.vram[addr as usize - 0x8000] = val,
            0xA000..=0xBFFF => self.cart.write_ram(addr, val),
            0xC000..=0xDFFF => self.wram[addr as usize - 0xC000] = val,
            0xE000..=0xFDFF => self.wram[addr as usize - 0xE000] = val,
            0xFE00..=0xFE9F => self.ppu.oam[addr as usize - 0xFE00] = val,
            0xFEA0..=0xFEFF => {}
            0xFF00 => self.joypad.write(val),
            0xFF01 => self.serial.sb = val,
            0xFF02 => self.serial.write_sc(val),
            0xFF04 => {
                let old_div = self.timer.div_counter();
                self.timer.write_div();
                self.serial.tick(old_div, 0, &mut self.if_reg);
            }
            0xFF05 => self.timer.write_tima(val),
            0xFF06 => self.timer.write_tma(val),
            0xFF07 => self.timer.write_tac(val),
            0xFF0F => self.if_reg = val | 0xE0,
            0xFF40 => self.ppu.write_lcdc(val),
            0xFF41 => self.ppu.write_stat(val),
            0xFF42 => self.ppu.scy = val,
            0xFF43 => self.ppu.scx = val,
            0xFF44 => {}
            0xFF45 => self.ppu.lyc = val,
            0xFF46 => self.dma.start(val),
            0xFF47 => self.ppu.bgp = val,
            0xFF48 => self.ppu.obp0 = val,
            0xFF49 => self.ppu.obp1 = val,
            0xFF4A => self.ppu.wy = val,
            0xFF4B => self.ppu.wx = val,
            0xFF80..=0xFFFE => self.hram[addr as usize - 0xFF80] = val,
            0xFFFF => self.ie = val,
            _ => {}
        }
    }

    pub fn interrupt_pending(&self) -> u8 {
        self.ie & self.if_reg & 0x1F
    }

    pub fn request_interrupt(&mut self, bit: u8) {
        self.if_reg |= bit;
    }
}
