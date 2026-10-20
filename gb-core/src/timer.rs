pub struct Timer {
    div_counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    reload_delay: Option<u8>,
}

impl Timer {
    pub fn new() -> Self {
        Timer {
            div_counter: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            reload_delay: None,
        }
    }

    pub fn div(&self) -> u8 {
        (self.div_counter >> 8) as u8
    }

    pub fn write_div(&mut self, if_reg: &mut u8) {
        let old = self.timer_signal();
        self.div_counter = 0;
        let new = self.timer_signal();
        if old && !new {
            self.increment_tima(if_reg);
        }
    }

    pub fn write_tac(&mut self, val: u8, if_reg: &mut u8) {
        let old = self.timer_signal();
        self.tac = val & 0x07;
        let new = self.timer_signal();
        if old && !new {
            self.increment_tima(if_reg);
        }
    }

    fn timer_signal(&self) -> bool {
        if self.tac & 0x04 == 0 {
            return false;
        }
        let bit = match self.tac & 0x03 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        };
        self.div_counter & (1 << bit) != 0
    }

    fn increment_tima(&mut self, if_reg: &mut u8) {
        let (v, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = 0;
            self.reload_delay = Some(4);
            let _ = if_reg;
        } else {
            self.tima = v;
        }
    }

    pub fn tick(&mut self, if_reg: &mut u8) {
        if let Some(n) = self.reload_delay {
            if n <= 1 {
                self.tima = self.tma;
                *if_reg |= 0x04;
                self.reload_delay = None;
            } else {
                self.reload_delay = Some(n - 1);
            }
        }
        let old = self.timer_signal();
        self.div_counter = self.div_counter.wrapping_add(1);
        let new = self.timer_signal();
        if old && !new {
            self.increment_tima(if_reg);
        }
    }
}
