#[derive(Default)]
pub struct Joypad {
    select: u8,
    buttons: u8,
    directions: u8,
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            select: 0x30,
            buttons: 0x0F,
            directions: 0x0F,
        }
    }

    pub fn read(&self) -> u8 {
        let mut low = 0x0F;
        if self.select & 0x10 == 0 {
            low &= self.directions;
        }
        if self.select & 0x20 == 0 {
            low &= self.buttons;
        }
        0xC0 | self.select | low
    }

    pub fn write(&mut self, val: u8) {
        self.select = val & 0x30;
    }

    pub fn press(&mut self, button: u8, is_button_group: bool, if_reg: &mut u8) {
        let before = self.read() & 0x0F;
        if is_button_group {
            self.buttons &= !(1 << button);
        } else {
            self.directions &= !(1 << button);
        }
        let after = self.read() & 0x0F;
        if before & !after != 0 {
            *if_reg |= 0x10;
        }
    }

    pub fn release(&mut self, button: u8, is_button_group: bool) {
        if is_button_group {
            self.buttons |= 1 << button;
        } else {
            self.directions |= 1 << button;
        }
    }
}
