/// Serial port (SB/SC), modeled far enough for test-ROM output capture and
/// correct internal-clock transfer timing. There is no link partner: bits
/// shifted in are always 1.
///
/// With the internal clock selected, one bit is shifted per 8192 Hz serial
/// clock period, derived from the system counter: on each falling edge of bit 8
/// of the DIV counter (every 512 T-cycles). A transfer takes 8 such edges.
pub struct Serial {
    pub sb: u8,
    pub sc: u8,
    /// Every byte the program started transmitting, in order.
    pub output: Vec<u8>,
    bits_left: u8,
}

const SERIAL_CLOCK_BIT: u16 = 1 << 8;

impl Serial {
    pub fn new() -> Self {
        Serial {
            sb: 0,
            sc: 0x7E,
            output: Vec::new(),
            bits_left: 0,
        }
    }

    pub fn read_sc(&self) -> u8 {
        self.sc | 0x7E
    }

    pub fn write_sc(&mut self, val: u8) {
        self.sc = val;
        if val & 0x80 != 0 {
            self.output.push(self.sb);
            self.bits_left = 8;
        } else {
            self.bits_left = 0;
        }
    }

    /// Called every T-cycle with the DIV counter before and after it changed.
    pub fn tick(&mut self, old_div: u16, new_div: u16, if_reg: &mut u8) {
        if self.bits_left == 0 || self.sc & 0x81 != 0x81 {
            return;
        }
        if old_div & SERIAL_CLOCK_BIT != 0 && new_div & SERIAL_CLOCK_BIT == 0 {
            self.sb = (self.sb << 1) | 1;
            self.bits_left -= 1;
            if self.bits_left == 0 {
                self.sc &= 0x7F;
                *if_reg |= 0x08;
            }
        }
    }
}

impl Default for Serial {
    fn default() -> Self {
        Self::new()
    }
}
