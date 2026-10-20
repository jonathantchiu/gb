use crate::{SCREEN_H, SCREEN_W};

pub struct Ppu {
    pub lcdc: u8,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    pub vram: [u8; 0x2000],
    pub oam: [u8; 0xA0],
    pub framebuffer: Vec<u8>,
    dot: u16,
    stat_irq_line: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            lcdc: 0x91,
            stat: 0x85,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            vram: [0; 0x2000],
            oam: [0; 0xA0],
            framebuffer: vec![0; SCREEN_W * SCREEN_H],
            dot: 0,
            stat_irq_line: false,
        }
    }

    pub fn mode(&self) -> u8 {
        if self.lcdc & 0x80 == 0 {
            0
        } else if self.ly >= 144 {
            1
        } else if self.dot < 80 {
            2
        } else if self.dot < 252 {
            3
        } else {
            0
        }
    }

    pub fn read_stat(&self) -> u8 {
        let coincidence = if self.ly == self.lyc { 0x04 } else { 0 };
        (self.stat & 0x78) | 0x80 | coincidence | self.mode()
    }

    pub fn write_stat(&mut self, val: u8) {
        self.stat = val & 0x78;
    }

    pub fn read_lcdc(&self) -> u8 {
        self.lcdc
    }

    pub fn write_lcdc(&mut self, val: u8) {
        let was_on = self.lcdc & 0x80 != 0;
        let now_on = val & 0x80 != 0;
        self.lcdc = val;
        if was_on && !now_on {
            self.ly = 0;
            self.dot = 0;
        }
    }

    pub fn write_ly(&mut self) {
        // LY is read-only; writes are ignored.
    }

    pub fn tick(&mut self, if_reg: &mut u8) {
        if self.lcdc & 0x80 == 0 {
            self.stat_irq_line = false;
            return;
        }
        let prev_mode = self.mode();
        self.dot += 1;
        if self.dot == 456 {
            self.dot = 0;
            self.ly += 1;
            if self.ly == 154 {
                self.ly = 0;
            }
        }
        let mode = self.mode();

        if prev_mode != 1 && mode == 1 {
            *if_reg |= 0x01;
        }

        let mut line = false;
        if self.ly == self.lyc {
            line = self.stat & 0x40 != 0;
        }
        let mode_ie = match mode {
            0 => self.stat & 0x08 != 0,
            1 => self.stat & 0x10 != 0,
            2 => self.stat & 0x20 != 0,
            _ => false,
        };
        if prev_mode != mode {
            line = line || mode_ie;
        }
        if line && !self.stat_irq_line {
            *if_reg |= 0x02;
        }
        self.stat_irq_line = line;
    }
}
