use crate::cartridge::Cartridge;
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

pub struct Bus {
    pub cart: Cartridge,
    pub ppu: Ppu,
    pub timer: Timer,
    pub serial: Serial,
    pub joypad: Joypad,
    pub wram: [u8; 0x2000],
    pub hram: [u8; 0x7F],
    pub ie: u8,
    pub if_reg: u8,
}

impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        Bus {
            cart,
            ppu: Ppu::new(),
            timer: Timer::new(),
            serial: Serial::new(),
            joypad: Joypad::new(),
            wram: [0; 0x2000],
            hram: [0; 0x7F],
            ie: 0,
            if_reg: 0xE1,
        }
    }

    pub fn tick_mcycle(&mut self) {
        for _ in 0..4 {
            self.timer.tick(&mut self.if_reg);
            self.ppu.tick(&mut self.if_reg);
        }
    }

    pub fn read8(&mut self, addr: u16) -> u8 {
        let v = self.peek8(addr);
        self.tick_mcycle();
        v
    }

    pub fn write8(&mut self, addr: u16, val: u8) {
        self.poke8(addr, val);
        self.tick_mcycle();
    }

    pub fn peek8(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[addr as usize - 0x8000],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[addr as usize - 0xC000],
            0xE000..=0xFDFF => self.wram[addr as usize - 0xE000],
            0xFE00..=0xFE9F => self.ppu.oam[addr as usize - 0xFE00],
            0xFEA0..=0xFEFF => 0xFF,
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
            0xFF46 => 0xFF,
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
            0xFF02 => {
                let mut if_reg = self.if_reg;
                self.serial.write_sc(val, &mut if_reg);
                self.if_reg = if_reg;
            }
            0xFF04 => {
                let mut if_reg = self.if_reg;
                self.timer.write_div(&mut if_reg);
                self.if_reg = if_reg;
            }
            0xFF05 => self.timer.tima = val,
            0xFF06 => self.timer.tma = val,
            0xFF07 => {
                let mut if_reg = self.if_reg;
                self.timer.write_tac(val, &mut if_reg);
                self.if_reg = if_reg;
            }
            0xFF0F => self.if_reg = val | 0xE0,
            0xFF40 => self.ppu.write_lcdc(val),
            0xFF41 => self.ppu.write_stat(val),
            0xFF42 => self.ppu.scy = val,
            0xFF43 => self.ppu.scx = val,
            0xFF44 => self.ppu.write_ly(),
            0xFF45 => self.ppu.lyc = val,
            0xFF46 => self.oam_dma(val),
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

    fn oam_dma(&mut self, val: u8) {
        let src = (val as u16) << 8;
        for i in 0..0xA0u16 {
            let b = self.peek8(src.wrapping_add(i));
            self.ppu.oam[i as usize] = b;
        }
    }

    pub fn interrupt_pending(&self) -> u8 {
        self.ie & self.if_reg & 0x1F
    }

    pub fn request_interrupt(&mut self, bit: u8) {
        self.if_reg |= bit;
    }
}
