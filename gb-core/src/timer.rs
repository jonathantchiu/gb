/// DIV/TIMA/TMA/TAC.
///
/// DIV is the upper byte of a free-running 16-bit counter that advances every
/// T-cycle. TIMA increments on the falling edge of (selected DIV bit AND timer
/// enable), so writes to DIV or TAC can themselves cause an increment.
///
/// On overflow TIMA reads 00 for 4 T-cycles, then is loaded from TMA and the
/// timer interrupt is requested. In terms of CPU-visible M-cycles:
/// - during the M-cycle(s) where TIMA reads 00 ("cycle A"), writing TIMA
///   cancels the reload and the interrupt;
/// - during the M-cycle after the reload ("cycle B"), writing TIMA is
///   ignored, and writing TMA also lands in TIMA.
///
/// Counter-driven overflows happen on the last T-cycle of an M-cycle, so the
/// reload lands at the end of the next one. A DIV/TAC write happens at the
/// CPU access point, before that M-cycle's T-cycles, so its reload lands at
/// the end of the same M-cycle. Both are "4 T-cycles after overflow".
pub struct Timer {
    div_counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    /// T-cycles until TIMA is reloaded from TMA (cycle A while nonzero).
    reload_in: u8,
    cycle_b: CycleB,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CycleB {
    No,
    /// Reloaded during this M-cycle; cycle B is the next M-cycle.
    Next,
    Now,
}

const RELOAD_DELAY: u8 = 4;

impl Timer {
    pub fn new() -> Self {
        Timer {
            div_counter: 0xABCC,
            tima: 0,
            tma: 0,
            tac: 0,
            reload_in: 0,
            cycle_b: CycleB::No,
        }
    }

    pub fn with_div_counter(mut self, counter: u16) -> Self {
        self.div_counter = counter;
        self
    }

    pub fn div_counter(&self) -> u16 {
        self.div_counter
    }

    pub fn div(&self) -> u8 {
        (self.div_counter >> 8) as u8
    }

    fn signal(&self) -> bool {
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

    fn set_with_edge(&mut self, f: impl FnOnce(&mut Self)) {
        let old = self.signal();
        f(self);
        if old && !self.signal() {
            let (v, overflow) = self.tima.overflowing_add(1);
            self.tima = v;
            if overflow {
                self.reload_in = RELOAD_DELAY;
            }
        }
    }

    /// Called at the start of every M-cycle, before the CPU's bus access.
    pub fn begin_mcycle(&mut self) {
        self.cycle_b = match self.cycle_b {
            CycleB::Next => CycleB::Now,
            _ => CycleB::No,
        };
    }

    /// Advances one T-cycle.
    pub fn tick(&mut self, if_reg: &mut u8) {
        if self.reload_in > 0 {
            self.reload_in -= 1;
            if self.reload_in == 0 {
                self.tima = self.tma;
                *if_reg |= 0x04;
                self.cycle_b = CycleB::Next;
            }
        }
        self.set_with_edge(|t| t.div_counter = t.div_counter.wrapping_add(1));
    }

    pub fn write_div(&mut self) {
        self.set_with_edge(|t| t.div_counter = 0);
    }

    pub fn write_tac(&mut self, val: u8) {
        self.set_with_edge(|t| t.tac = val & 0x07);
    }

    pub fn write_tima(&mut self, val: u8) {
        if self.cycle_b == CycleB::Now {
            return;
        }
        self.reload_in = 0;
        self.tima = val;
    }

    pub fn write_tma(&mut self, val: u8) {
        self.tma = val;
        if self.cycle_b == CycleB::Now {
            self.tima = val;
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
