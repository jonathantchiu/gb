pub mod apu;
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

/// T-cycles per second on DMG.
pub const CLOCK_HZ: u32 = 4_194_304;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

impl Button {
    /// (bit index within the P1 nibble, is in the action-button group)
    fn wiring(self) -> (u8, bool) {
        match self {
            Button::Right => (0, false),
            Button::Left => (1, false),
            Button::Up => (2, false),
            Button::Down => (3, false),
            Button::A => (0, true),
            Button::B => (1, true),
            Button::Select => (2, true),
            Button::Start => (3, true),
        }
    }
}

#[derive(Clone)]
pub struct GameBoy {
    pub cpu: Cpu,
    pub bus: Bus,
}

impl GameBoy {
    /// Default machine: synthesized post-boot state, no boot ROM.
    pub fn new(rom: &[u8]) -> Self {
        let cart = Cartridge::from_rom(rom);
        GameBoy {
            cpu: Cpu::new_post_boot(),
            bus: Bus::new(cart),
        }
    }

    /// With a user-supplied boot ROM: the CPU starts at 0 with the boot ROM
    /// mapped; the boot ROM unmaps itself via FF50. Without one, synthesized
    /// post-boot state (no Nintendo code in the repo).
    pub fn with_boot_rom(rom: &[u8], boot_rom: Option<Vec<u8>>) -> Self {
        let cart = Cartridge::from_rom(rom);
        match boot_rom {
            Some(boot) => GameBoy {
                cpu: Cpu::new_boot_rom(),
                bus: Bus::with_boot_rom(cart, Some(boot)),
            },
            None => GameBoy::new(rom),
        }
    }

    /// Executes one instruction (or one interrupt dispatch, or one halted
    /// M-cycle). Returns the number of M-cycles that elapsed.
    pub fn step(&mut self) -> u32 {
        let before = self.bus.mcycles;
        self.cpu.step(&mut self.bus);
        (self.bus.mcycles - before) as u32
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

    pub fn take_audio_samples(&mut self) -> Vec<(i16, i16)> {
        std::mem::take(&mut self.bus.apu.audio_buffer)
    }

    /// Runs until the PPU finishes the next frame (enters VBlank). With the
    /// LCD off there are no frames, so fall back to one frame's worth of time.
    pub fn run_frame(&mut self) {
        let target = self.bus.ppu.frame_count + 1;
        let deadline = self.bus.mcycles + 70224 / 4;
        while self.bus.ppu.frame_count < target && self.bus.mcycles < deadline {
            self.step();
        }
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let (bit, group) = button.wiring();
        if pressed {
            self.bus.joypad.press(bit, group, &mut self.bus.if_reg);
        } else {
            self.bus.joypad.release(bit, group);
        }
    }

    pub fn title(&self) -> &str {
        &self.bus.cart.title
    }

    pub fn has_battery(&self) -> bool {
        self.bus.cart.has_battery
    }

    pub fn save_data(&self) -> Vec<u8> {
        self.bus.cart.save_data()
    }

    pub fn load_save_data(&mut self, data: &[u8]) {
        self.bus.cart.load_save_data(data);
    }
}
