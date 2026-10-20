pub enum Mapper {
    NoMbc,
    Mbc1 {
        rom_bank: u8,
        bank2: u8,
        mode: u8,
        ram_enabled: bool,
    },
    Mbc5 {
        rom_bank: u16,
        ram_bank: u8,
        ram_enabled: bool,
    },
}

pub struct Cartridge {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub mapper: Mapper,
    pub title: String,
}

impl Cartridge {
    pub fn from_rom(rom: &[u8]) -> Self {
        let mut data = rom.to_vec();
        if data.len() < 0x8000 {
            data.resize(0x8000, 0);
        }
        let cart_type = data[0x147];
        let ram_size = match data[0x149] {
            0x00 => 0,
            0x01 => 2 * 1024,
            0x02 => 8 * 1024,
            0x03 => 32 * 1024,
            0x04 => 128 * 1024,
            0x05 => 64 * 1024,
            _ => 0,
        };
        let mapper = match cart_type {
            0x00 | 0x08 | 0x09 => Mapper::NoMbc,
            0x01..=0x03 => Mapper::Mbc1 {
                rom_bank: 1,
                bank2: 0,
                mode: 0,
                ram_enabled: false,
            },
            0x19..=0x1E => Mapper::Mbc5 {
                rom_bank: 1,
                ram_bank: 0,
                ram_enabled: false,
            },
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
        }
    }

    fn rom_bank_count(&self) -> usize {
        (self.rom.len() / 0x4000).max(1)
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        let addr = addr as usize;
        match &self.mapper {
            Mapper::NoMbc => self.rom.get(addr).copied().unwrap_or(0xFF),
            Mapper::Mbc1 {
                rom_bank,
                bank2,
                mode,
                ..
            } => {
                let banks = self.rom_bank_count();
                let idx = if addr < 0x4000 {
                    let b = if *mode == 1 {
                        ((*bank2 as usize) << 5) % banks
                    } else {
                        0
                    };
                    b * 0x4000 + addr
                } else {
                    let low = if rom_bank & 0x1F == 0 { 1 } else { *rom_bank & 0x1F };
                    let b = (((*bank2 as usize) << 5) | low as usize) % banks;
                    b * 0x4000 + (addr - 0x4000)
                };
                self.rom.get(idx).copied().unwrap_or(0xFF)
            }
            Mapper::Mbc5 { rom_bank, .. } => {
                let banks = self.rom_bank_count();
                let idx = if addr < 0x4000 {
                    addr
                } else {
                    (*rom_bank as usize % banks) * 0x4000 + (addr - 0x4000)
                };
                self.rom.get(idx).copied().unwrap_or(0xFF)
            }
        }
    }

    pub fn write_rom(&mut self, addr: u16, val: u8) {
        match &mut self.mapper {
            Mapper::NoMbc => {}
            Mapper::Mbc1 {
                rom_bank,
                bank2,
                mode,
                ram_enabled,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = val & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    *rom_bank = val & 0x1F;
                    if *rom_bank == 0 {
                        *rom_bank = 1;
                    }
                }
                0x4000..=0x5FFF => *bank2 = val & 0x03,
                0x6000..=0x7FFF => *mode = val & 0x01,
                _ => {}
            },
            Mapper::Mbc5 {
                rom_bank,
                ram_bank,
                ram_enabled,
            } => match addr {
                0x0000..=0x1FFF => *ram_enabled = val & 0x0F == 0x0A,
                0x2000..=0x2FFF => *rom_bank = (*rom_bank & 0x100) | val as u16,
                0x3000..=0x3FFF => *rom_bank = (*rom_bank & 0xFF) | (((val & 1) as u16) << 8),
                0x4000..=0x5FFF => *ram_bank = val & 0x0F,
                _ => {}
            },
        }
    }

    fn ram_addr(&self, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        let bank = match &self.mapper {
            Mapper::Mbc1 { bank2, mode, .. } if *mode == 1 => *bank2 as usize,
            Mapper::Mbc5 { ram_bank, .. } => *ram_bank as usize,
            _ => 0,
        };
        let idx = bank * 0x2000 + (addr as usize - 0xA000);
        if idx < self.ram.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn ram_enabled(&self) -> bool {
        match &self.mapper {
            Mapper::NoMbc => true,
            Mapper::Mbc1 { ram_enabled, .. } => *ram_enabled,
            Mapper::Mbc5 { ram_enabled, .. } => *ram_enabled,
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled() {
            return 0xFF;
        }
        match self.ram_addr(addr) {
            Some(i) => self.ram[i],
            None => 0xFF,
        }
    }

    pub fn write_ram(&mut self, addr: u16, val: u8) {
        if !self.ram_enabled() {
            return;
        }
        if let Some(i) = self.ram_addr(addr) {
            self.ram[i] = val;
        }
    }
}
