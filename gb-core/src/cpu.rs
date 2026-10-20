use crate::bus::Bus;

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    ime_scheduled: bool,
    pub halted: bool,
    halt_bug: bool,
    pub stopped: bool,
}

impl Cpu {
    pub fn new_post_boot() -> Self {
        Cpu {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            ime_scheduled: false,
            halted: false,
            halt_bug: false,
            stopped: false,
        }
    }

    fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | self.c as u16
    }
    fn de(&self) -> u16 {
        ((self.d as u16) << 8) | self.e as u16
    }
    fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | self.l as u16
    }
    fn af(&self) -> u16 {
        ((self.a as u16) << 8) | self.f as u16
    }
    fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }
    fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }
    fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }
    fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = v as u8 & 0xF0;
    }

    fn set_flag(&mut self, flag: u8, on: bool) {
        if on {
            self.f |= flag;
        } else {
            self.f &= !flag;
        }
        self.f &= 0xF0;
    }
    fn flag(&self, flag: u8) -> bool {
        self.f & flag != 0
    }

    fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.read8(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        v
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    fn push16(&mut self, bus: &mut Bus, v: u16) {
        bus.tick_mcycle();
        self.sp = self.sp.wrapping_sub(1);
        bus.write8(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write8(self.sp, v as u8);
    }

    fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let lo = bus.read8(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read8(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    fn reg8(&self, idx: u8) -> u8 {
        match idx {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            7 => self.a,
            _ => unreachable!(),
        }
    }

    fn set_reg8(&mut self, idx: u8, v: u8) {
        match idx {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            7 => self.a = v,
            _ => unreachable!(),
        }
    }

    fn read_r(&mut self, bus: &mut Bus, idx: u8) -> u8 {
        if idx == 6 {
            bus.read8(self.hl())
        } else {
            self.reg8(idx)
        }
    }

    fn write_r(&mut self, bus: &mut Bus, idx: u8, v: u8) {
        if idx == 6 {
            bus.write8(self.hl(), v);
        } else {
            self.set_reg8(idx, v);
        }
    }

    // ALU primitives. All flag semantics per SM83.
    fn alu_add(&mut self, v: u8, carry: bool) {
        let c = if carry && self.flag(FLAG_C) { 1 } else { 0 };
        let (r, co1) = self.a.overflowing_add(v);
        let (r, co2) = r.overflowing_add(c);
        let h = (self.a & 0x0F) + (v & 0x0F) + c > 0x0F;
        self.f = 0;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_H, h);
        self.set_flag(FLAG_C, co1 || co2);
        self.a = r;
    }

    fn alu_sub(&mut self, v: u8, carry: bool, keep: bool) {
        let c = if carry && self.flag(FLAG_C) { 1 } else { 0 };
        let (r, bo1) = self.a.overflowing_sub(v);
        let (r, bo2) = r.overflowing_sub(c);
        let h = (self.a & 0x0F) < (v & 0x0F) + c;
        let old_a = self.a;
        self.f = 0;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, h);
        self.set_flag(FLAG_C, bo1 || bo2);
        self.a = if keep { old_a } else { r };
    }

    fn alu_and(&mut self, v: u8) {
        self.a &= v;
        self.f = 0;
        self.set_flag(FLAG_Z, self.a == 0);
        self.set_flag(FLAG_H, true);
    }

    fn alu_xor(&mut self, v: u8) {
        self.a ^= v;
        self.f = 0;
        self.set_flag(FLAG_Z, self.a == 0);
    }

    fn alu_or(&mut self, v: u8) {
        self.a |= v;
        self.f = 0;
        self.set_flag(FLAG_Z, self.a == 0);
    }

    fn inc8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        let c = self.flag(FLAG_C);
        self.f = 0;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_H, v & 0x0F == 0x0F);
        self.set_flag(FLAG_C, c);
        r
    }

    fn dec8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        let c = self.flag(FLAG_C);
        self.f = 0;
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, v & 0x0F == 0);
        self.set_flag(FLAG_C, c);
        r
    }

    fn add_hl(&mut self, v: u16) {
        let hl = self.hl();
        let r = hl.wrapping_add(v);
        let z = self.flag(FLAG_Z);
        self.f = 0;
        self.set_flag(FLAG_Z, z);
        self.set_flag(FLAG_H, (hl & 0x0FFF) + (v & 0x0FFF) > 0x0FFF);
        self.set_flag(FLAG_C, (hl as u32) + (v as u32) > 0xFFFF);
        self.set_hl(r);
    }

    fn add_sp_e8(&mut self, e: i8) -> u16 {
        let sp = self.sp;
        let r = sp.wrapping_add(e as u16);
        let eu = e as u8 as u16;
        self.f = 0;
        self.set_flag(FLAG_H, (sp & 0x0F) + (eu & 0x0F) > 0x0F);
        self.set_flag(FLAG_C, (sp & 0xFF) + (eu & 0xFF) > 0xFF);
        r
    }

    fn daa(&mut self) {
        let mut adj = 0u8;
        let mut c = self.flag(FLAG_C);
        if self.flag(FLAG_N) {
            if self.flag(FLAG_H) {
                adj |= 0x06;
            }
            if c {
                adj |= 0x60;
            }
            self.a = self.a.wrapping_sub(adj);
        } else {
            if self.flag(FLAG_H) || self.a & 0x0F > 0x09 {
                adj |= 0x06;
            }
            if c || self.a > 0x99 {
                adj |= 0x60;
                c = true;
            }
            self.a = self.a.wrapping_add(adj);
        }
        let z = self.a == 0;
        let n = self.flag(FLAG_N);
        self.f = 0;
        self.set_flag(FLAG_Z, z);
        self.set_flag(FLAG_N, n);
        self.set_flag(FLAG_C, c);
    }

    fn jr_cond(&mut self, bus: &mut Bus, cond: bool) {
        let e = self.fetch8(bus) as i8;
        if cond {
            bus.tick_mcycle();
            self.pc = self.pc.wrapping_add(e as u16);
        }
    }

    fn jp_cond(&mut self, bus: &mut Bus, cond: bool) {
        let addr = self.fetch16(bus);
        if cond {
            bus.tick_mcycle();
            self.pc = addr;
        }
    }

    fn call_cond(&mut self, bus: &mut Bus, cond: bool) {
        let addr = self.fetch16(bus);
        if cond {
            self.push16(bus, self.pc);
            self.pc = addr;
        }
    }

    fn ret_cond(&mut self, bus: &mut Bus, cond: bool) {
        bus.tick_mcycle();
        if cond {
            let addr = self.pop16(bus);
            bus.tick_mcycle();
            self.pc = addr;
        }
    }

    pub fn step(&mut self, bus: &mut Bus) {
        let pending = bus.interrupt_pending();

        if self.halted && pending != 0 {
            self.halted = false;
        }

        if self.ime && pending != 0 {
            self.service_interrupt(bus);
            return;
        }

        if self.halted || self.stopped {
            bus.tick_mcycle();
            return;
        }

        let was_scheduled = self.ime_scheduled;
        let op = self.fetch8(bus);
        self.execute(bus, op);

        // EI takes effect after the following instruction, unless that
        // instruction was DI (which clears the schedule).
        if was_scheduled && self.ime_scheduled {
            self.ime = true;
            self.ime_scheduled = false;
        }
    }

    /// Interrupt dispatch: 5 M-cycles. Which interrupt to service is decided
    /// after the high byte of PC is pushed, so a push that overwrites IE can
    /// change the target, or cancel dispatch entirely (PC = 0x0000).
    fn service_interrupt(&mut self, bus: &mut Bus) {
        self.ime = false;
        self.ime_scheduled = false;
        self.halted = false;
        bus.tick_mcycle();
        bus.tick_mcycle();
        let pc = self.pc;
        self.sp = self.sp.wrapping_sub(1);
        bus.write8(self.sp, (pc >> 8) as u8);
        let pending = bus.interrupt_pending();
        self.pc = if pending == 0 {
            0x0000
        } else {
            let bit = pending.trailing_zeros() as u16;
            bus.if_reg &= !(1 << bit);
            0x0040 + 8 * bit
        };
        self.sp = self.sp.wrapping_sub(1);
        bus.write8(self.sp, pc as u8);
        bus.tick_mcycle();
    }

    fn execute(&mut self, bus: &mut Bus, op: u8) {
        match op {
            0x00 => {}
            0x10 => {
                self.fetch8(bus);
                self.stopped = true;
            }
            0x76 => {
                let pending = bus.interrupt_pending();
                if !self.ime && pending != 0 {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
            }

            // 8-bit immediate loads
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => {
                let v = self.fetch8(bus);
                self.set_reg8(op >> 3, v);
            }
            0x36 => {
                let v = self.fetch8(bus);
                bus.write8(self.hl(), v);
            }

            // LD r,r' / LD r,(HL) / LD (HL),r grid
            0x40..=0x7F => {
                let dst = (op >> 3) & 7;
                let src = op & 7;
                let v = self.read_r(bus, src);
                self.write_r(bus, dst, v);
            }

            // ALU A,r grid
            0x80..=0xBF => {
                let v = self.read_r(bus, op & 7);
                match (op >> 3) & 7 {
                    0 => self.alu_add(v, false),
                    1 => self.alu_add(v, true),
                    2 => self.alu_sub(v, false, false),
                    3 => self.alu_sub(v, true, false),
                    4 => self.alu_and(v),
                    5 => self.alu_xor(v),
                    6 => self.alu_or(v),
                    _ => self.alu_sub(v, false, true),
                }
            }

            // ALU A,n
            0xC6 => {
                let v = self.fetch8(bus);
                self.alu_add(v, false);
            }
            0xCE => {
                let v = self.fetch8(bus);
                self.alu_add(v, true);
            }
            0xD6 => {
                let v = self.fetch8(bus);
                self.alu_sub(v, false, false);
            }
            0xDE => {
                let v = self.fetch8(bus);
                self.alu_sub(v, true, false);
            }
            0xE6 => {
                let v = self.fetch8(bus);
                self.alu_and(v);
            }
            0xEE => {
                let v = self.fetch8(bus);
                self.alu_xor(v);
            }
            0xF6 => {
                let v = self.fetch8(bus);
                self.alu_or(v);
            }
            0xFE => {
                let v = self.fetch8(bus);
                self.alu_sub(v, false, true);
            }
            _ => self.execute_rest(bus, op),
        }
    }

    fn rotate(&mut self, kind: u8, v: u8, keep_z: bool) -> u8 {
        let old_c = self.flag(FLAG_C);
        let (r, c) = match kind {
            0 => (v.rotate_left(1), v & 0x80 != 0),
            1 => (v.rotate_right(1), v & 0x01 != 0),
            2 => ((v << 1) | old_c as u8, v & 0x80 != 0),
            3 => ((v >> 1) | ((old_c as u8) << 7), v & 0x01 != 0),
            4 => (v << 1, v & 0x80 != 0),
            5 => ((v >> 1) | (v & 0x80), v & 0x01 != 0),
            6 => (v.rotate_left(4), false),
            _ => (v >> 1, v & 0x01 != 0),
        };
        self.f = 0;
        self.set_flag(FLAG_Z, !keep_z && r == 0);
        self.set_flag(FLAG_C, c);
        r
    }

    fn execute_cb(&mut self, bus: &mut Bus, op: u8) {
        let idx = op & 7;
        let kind = (op >> 3) & 7;
        match op {
            0x00..=0x3F => {
                let v = self.read_r(bus, idx);
                let r = self.rotate(kind, v, false);
                self.write_r(bus, idx, r);
            }
            0x40..=0x7F => {
                let v = self.read_r(bus, idx);
                let c = self.flag(FLAG_C);
                self.f = 0;
                self.set_flag(FLAG_Z, v & (1 << kind) == 0);
                self.set_flag(FLAG_H, true);
                self.set_flag(FLAG_C, c);
            }
            0x80..=0xBF => {
                let v = self.read_r(bus, idx);
                self.write_r(bus, idx, v & !(1 << kind));
            }
            _ => {
                let v = self.read_r(bus, idx);
                self.write_r(bus, idx, v | (1 << kind));
            }
        }
    }

    fn cond(&self, cc: u8) -> bool {
        match cc {
            0 => !self.flag(FLAG_Z),
            1 => self.flag(FLAG_Z),
            2 => !self.flag(FLAG_C),
            _ => self.flag(FLAG_C),
        }
    }

    fn rp2(&mut self, idx: u8) -> u16 {
        match idx {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.af(),
        }
    }

    fn set_rp2(&mut self, idx: u8, v: u16) {
        match idx {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_hl(v),
            _ => self.set_af(v),
        }
    }

    fn execute_rest(&mut self, bus: &mut Bus, op: u8) {
        match op {
            0x01 | 0x11 | 0x21 | 0x31 => {
                let v = self.fetch16(bus);
                match op >> 4 {
                    0 => self.set_bc(v),
                    1 => self.set_de(v),
                    2 => self.set_hl(v),
                    _ => self.sp = v,
                }
            }
            0x02 => bus.write8(self.bc(), self.a),
            0x12 => bus.write8(self.de(), self.a),
            0x0A => self.a = bus.read8(self.bc()),
            0x1A => self.a = bus.read8(self.de()),
            0x22 => {
                let hl = self.hl();
                bus.write8(hl, self.a);
                self.set_hl(hl.wrapping_add(1));
            }
            0x2A => {
                let hl = self.hl();
                self.a = bus.read8(hl);
                self.set_hl(hl.wrapping_add(1));
            }
            0x32 => {
                let hl = self.hl();
                bus.write8(hl, self.a);
                self.set_hl(hl.wrapping_sub(1));
            }
            0x3A => {
                let hl = self.hl();
                self.a = bus.read8(hl);
                self.set_hl(hl.wrapping_sub(1));
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                bus.tick_mcycle();
                match op >> 4 {
                    0 => self.set_bc(self.bc().wrapping_add(1)),
                    1 => self.set_de(self.de().wrapping_add(1)),
                    2 => self.set_hl(self.hl().wrapping_add(1)),
                    _ => self.sp = self.sp.wrapping_add(1),
                }
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                bus.tick_mcycle();
                match op >> 4 {
                    0 => self.set_bc(self.bc().wrapping_sub(1)),
                    1 => self.set_de(self.de().wrapping_sub(1)),
                    2 => self.set_hl(self.hl().wrapping_sub(1)),
                    _ => self.sp = self.sp.wrapping_sub(1),
                }
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                bus.tick_mcycle();
                let v = match op >> 4 {
                    0 => self.bc(),
                    1 => self.de(),
                    2 => self.hl(),
                    _ => self.sp,
                };
                self.add_hl(v);
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x3C => {
                let idx = (op >> 3) & 7;
                let v = self.inc8(self.reg8(idx));
                self.set_reg8(idx, v);
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x3D => {
                let idx = (op >> 3) & 7;
                let v = self.dec8(self.reg8(idx));
                self.set_reg8(idx, v);
            }
            0x34 => {
                let v = bus.read8(self.hl());
                let r = self.inc8(v);
                bus.write8(self.hl(), r);
            }
            0x35 => {
                let v = bus.read8(self.hl());
                let r = self.dec8(v);
                bus.write8(self.hl(), r);
            }
            0x27 => self.daa(),
            0x2F => {
                self.a = !self.a;
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, true);
            }
            0x37 => {
                let z = self.flag(FLAG_Z);
                self.f = 0;
                self.set_flag(FLAG_Z, z);
                self.set_flag(FLAG_C, true);
            }
            0x3F => {
                let z = self.flag(FLAG_Z);
                let c = self.flag(FLAG_C);
                self.f = 0;
                self.set_flag(FLAG_Z, z);
                self.set_flag(FLAG_C, !c);
            }
            0x07 => self.a = self.rotate(0, self.a, true),
            0x0F => self.a = self.rotate(1, self.a, true),
            0x17 => self.a = self.rotate(2, self.a, true),
            0x1F => self.a = self.rotate(3, self.a, true),
            0x18 => self.jr_cond(bus, true),
            0x20 | 0x28 | 0x30 | 0x38 => {
                let cc = (op >> 3) & 3;
                let c = self.cond(cc);
                self.jr_cond(bus, c);
            }
            0xC3 => self.jp_cond(bus, true),
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let cc = (op >> 3) & 3;
                let c = self.cond(cc);
                self.jp_cond(bus, c);
            }
            0xE9 => self.pc = self.hl(),
            0xCD => self.call_cond(bus, true),
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                let cc = (op >> 3) & 3;
                let c = self.cond(cc);
                self.call_cond(bus, c);
            }
            0xC9 => {
                let addr = self.pop16(bus);
                bus.tick_mcycle();
                self.pc = addr;
            }
            0xD9 => {
                let addr = self.pop16(bus);
                bus.tick_mcycle();
                self.pc = addr;
                self.ime = true;
                self.ime_scheduled = false;
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                let cc = (op >> 3) & 3;
                let c = self.cond(cc);
                self.ret_cond(bus, c);
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let vec = (op & 0x38) as u16;
                self.push16(bus, self.pc);
                self.pc = vec;
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let v = self.pop16(bus);
                self.set_rp2((op >> 4) & 3, v);
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let v = self.rp2((op >> 4) & 3);
                self.push16(bus, v);
            }
            0xE0 => {
                let a = self.fetch8(bus);
                bus.write8(0xFF00 | a as u16, self.a);
            }
            0xF0 => {
                let a = self.fetch8(bus);
                self.a = bus.read8(0xFF00 | a as u16);
            }
            0xE2 => bus.write8(0xFF00 | self.c as u16, self.a),
            0xF2 => self.a = bus.read8(0xFF00 | self.c as u16),
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write8(addr, self.a);
            }
            0xFA => {
                let addr = self.fetch16(bus);
                self.a = bus.read8(addr);
            }
            0x08 => {
                let addr = self.fetch16(bus);
                bus.write8(addr, self.sp as u8);
                bus.write8(addr.wrapping_add(1), (self.sp >> 8) as u8);
            }
            0xE8 => {
                let e = self.fetch8(bus) as i8;
                bus.tick_mcycle();
                bus.tick_mcycle();
                self.sp = self.add_sp_e8(e);
            }
            0xF8 => {
                let e = self.fetch8(bus) as i8;
                bus.tick_mcycle();
                let r = self.add_sp_e8(e);
                self.set_hl(r);
            }
            0xF9 => {
                bus.tick_mcycle();
                self.sp = self.hl();
            }
            0xF3 => {
                self.ime = false;
                self.ime_scheduled = false;
            }
            0xFB => {
                self.ime_scheduled = true;
            }
            0xCB => {
                let cb = self.fetch8(bus);
                self.execute_cb(bus, cb);
            }
            _ => unreachable!("unimplemented opcode {op:#04x}"),
        }
    }
}
