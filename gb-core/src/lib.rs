pub mod bus;
pub mod cartridge;
pub mod cpu;
pub mod joypad;
pub mod ppu;
pub mod serial;
pub mod timer;

use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;

pub const SCREEN_W: usize = 160;
pub const SCREEN_H: usize = 144;

pub struct GameBoy {
    pub cpu: Cpu,
    pub bus: Bus,
}

impl GameBoy {
    pub fn new(rom: &[u8]) -> Self {
        let cart = Cartridge::from_rom(rom);
        GameBoy {
            cpu: Cpu::new_post_boot(),
            bus: Bus::new(cart),
        }
    }

    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    pub fn serial_output(&self) -> &[u8] {
        &self.bus.serial.output
    }

    pub fn take_serial_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bus.serial.output)
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer
    }
}
