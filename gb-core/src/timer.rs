#[derive(Clone, Copy, PartialEq, Eq)]
enum ReloadState {
    None,
    Reloading(u8),
    Reloaded(u8),
}

pub struct Timer {
    div_counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    reload_state: ReloadState,
}

impl Timer {
    pub fn new() -> Self {
        Timer {
            div_counter: 0xABCC,
            tima: 0,
            tma: 0,
            tac: 0,
            reload_state: ReloadState::None,
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

    pub fn write_tima(&mut self, val: u8) {
        match self.reload_state {
            ReloadState::Reloaded(_) => {}
            ReloadState::Reloading(_) => {
                self.tima = val;
                self.reload_state = ReloadState::None;
            }
            ReloadState::None => {
                self.tima = val;
            }
        }
    }

    pub fn write_tma(&mut self, val: u8) {
        self.tma = val;
        if matches!(self.reload_state, ReloadState::Reloaded(_)) {
            self.tima = val;
        }
    }

    pub fn write_tac(&mut self, val: u8, if_reg: &mut u8) {
        let prev_div = self.div_counter.wrapping_sub(1);
        let curr_div = self.div_counter;
        let old_tac = self.tac;
        let new_tac = val & 0x07;
        self.tac = new_tac;

        let sig_t2 = Self::signal_at(prev_div, old_tac);
        let sig_old_t4 = Self::signal_at(curr_div, old_tac);
        let sig_t3 = Self::signal_at(prev_div, new_tac);
        let sig_t4 = Self::signal_at(curr_div, new_tac);

        let had_edge = (sig_t2 && !sig_t3) || (sig_t3 && !sig_t4);
        let already_counted = sig_t2 && !sig_old_t4;
        if had_edge && !already_counted {
            self.increment_tima(if_reg);
        }
    }

    fn signal_at(div_counter: u16, tac: u8) -> bool {
        if tac & 0x04 == 0 {
            return false;
        }
        let bit = match tac & 0x03 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        };
        div_counter & (1 << bit) != 0
    }

    fn timer_signal(&self) -> bool {
        Self::signal_at(self.div_counter, self.tac)
    }

    fn increment_tima(&mut self, _if_reg: &mut u8) {
        let (v, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = 0;
            self.reload_state = ReloadState::Reloading(4);
        } else {
            self.tima = v;
        }
    }

    pub fn tick(&mut self, if_reg: &mut u8) {
        match self.reload_state {
            ReloadState::Reloading(n) => {
                if n <= 1 {
                    self.tima = self.tma;
                    *if_reg |= 0x04;
                    self.reload_state = ReloadState::Reloaded(4);
                } else {
                    self.reload_state = ReloadState::Reloading(n - 1);
                }
            }
            ReloadState::Reloaded(n) => {
                if n <= 1 {
                    self.reload_state = ReloadState::None;
                } else {
                    self.reload_state = ReloadState::Reloaded(n - 1);
                }
            }
            ReloadState::None => {}
        }

        let old = self.timer_signal();
        self.div_counter = self.div_counter.wrapping_add(1);
        let new = self.timer_signal();
        if old && !new {
            self.increment_tima(if_reg);
        }
    }
}
