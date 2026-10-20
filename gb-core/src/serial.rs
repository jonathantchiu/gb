pub struct Serial {
    pub sb: u8,
    pub sc: u8,
    pub output: Vec<u8>,
}

impl Serial {
    pub fn new() -> Self {
        Serial {
            sb: 0,
            sc: 0x7E,
            output: Vec::new(),
        }
    }

    pub fn read_sc(&self) -> u8 {
        self.sc | 0x7E
    }

    pub fn write_sc(&mut self, val: u8, if_reg: &mut u8) {
        self.sc = val;
        if val & 0x81 == 0x81 {
            self.output.push(self.sb);
            self.sb = 0xFF;
            self.sc &= 0x7F;
            *if_reg |= 0x08;
        }
    }
}
