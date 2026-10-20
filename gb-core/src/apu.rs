//! Game Boy Audio Processing Unit (APU).
//!
//! Steps at T-cycle granularity in the base (4.194 MHz) clock domain, with the
//! 512 Hz frame sequencer driven by falling edges of the system counter's
//! bit 12 (`DIV` bit 4 in single-speed mode).

const READ_MASKS: [u8; 23] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, // NR10-NR14
    0xFF, 0x3F, 0x00, 0xFF, 0xBF, // NR20-NR24
    0x7F, 0xFF, 0x9F, 0xFF, 0xBF, // NR30-NR34
    0xFF, 0xFF, 0x00, 0x00, 0xBF, // NR40-NR44
    0x00, 0x00, 0x70,             // NR50-NR52
];

const DUTY_TABLE: [u8; 4] = [
    0b0000_0001, // 12.5%
    0b1000_0001, // 25%
    0b1000_0111, // 50%
    0b0111_1110, // 75%
];

pub const SAMPLE_RATE: u32 = 48_000;
const CLOCK_HZ: u64 = 4_194_304;
const MAX_BUFFERED_SAMPLES: usize = 8192;

#[derive(Clone, Default)]
struct Envelope {
    initial_volume: u8,
    add_mode: bool,
    period: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn write_reg(&mut self, val: u8) {
        self.initial_volume = val >> 4;
        self.add_mode = (val & 0x08) != 0;
        self.period = val & 0x07;
    }

    fn dac_enabled(&self) -> bool {
        self.initial_volume != 0 || self.add_mode
    }

    fn trigger(&mut self) {
        self.volume = self.initial_volume;
        self.timer = if self.period == 0 { 8 } else { self.period };
    }

    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.add_mode && self.volume < 15 {
                self.volume += 1;
            } else if !self.add_mode && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

#[derive(Clone, Default)]
struct Sweep {
    period: u8,
    negate: bool,
    shift: u8,
    enabled: bool,
    shadow_freq: u16,
    timer: u8,
    negate_used: bool,
}

impl Sweep {
    fn write_nr10(&mut self, val: u8, ch1_enabled: &mut bool) {
        let new_negate = (val & 0x08) != 0;
        if self.negate && !new_negate && self.negate_used {
            *ch1_enabled = false;
        }
        self.period = (val >> 4) & 0x07;
        self.negate = new_negate;
        self.shift = val & 0x07;
    }

    fn calc(&mut self, ch1_enabled: &mut bool) -> u16 {
        let delta = self.shadow_freq >> self.shift;
        let new_freq = if self.negate {
            self.negate_used = true;
            self.shadow_freq.wrapping_sub(delta)
        } else {
            self.shadow_freq.wrapping_add(delta)
        };
        if new_freq > 2047 {
            *ch1_enabled = false;
        }
        new_freq
    }

    fn trigger(&mut self, freq: u16, ch1_enabled: &mut bool) {
        self.shadow_freq = freq;
        self.timer = if self.period == 0 { 8 } else { self.period };
        self.enabled = self.period != 0 || self.shift != 0;
        self.negate_used = false;
        if self.shift != 0 {
            self.calc(ch1_enabled);
        }
    }

    fn clock(&mut self, ch1_enabled: &mut bool) -> Option<u16> {
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = if self.period == 0 { 8 } else { self.period };
            if self.enabled && self.period != 0 {
                let new_freq = self.calc(ch1_enabled);
                if new_freq <= 2047 && self.shift != 0 {
                    self.shadow_freq = new_freq;
                    self.calc(ch1_enabled);
                    return Some(new_freq);
                }
            }
        }
        None
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Default)]
struct SquareChannel {
    enabled: bool,
    duty: u8,
    duty_pos: u8,
    length_counter: u16,
    len_enabled: bool,
    freq: u16,
    timer: u16,
    env: Envelope,
}

impl SquareChannel {
    fn clock_length(&mut self) {
        if self.len_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = (2048 - self.freq) * 4;
            self.duty_pos = (self.duty_pos + 1) & 7;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || !self.env.dac_enabled() {
            return 0;
        }
        let bit = (DUTY_TABLE[self.duty as usize] >> (7 - self.duty_pos)) & 1;
        if bit != 0 {
            self.env.volume
        } else {
            0
        }
    }
}

#[derive(Clone)]
struct WaveChannel {
    enabled: bool,
    dac_enabled: bool,
    length_counter: u16,
    len_enabled: bool,
    volume_code: u8,
    freq: u16,
    timer: u16,
    wave_pos: u8,
    sample_buffer: u8,
    ticks_since_read: u16,
}

impl Default for WaveChannel {
    fn default() -> Self {
        WaveChannel {
            enabled: false,
            dac_enabled: false,
            length_counter: 0,
            len_enabled: false,
            volume_code: 0,
            freq: 0,
            timer: 0,
            wave_pos: 0,
            sample_buffer: 0,
            ticks_since_read: u16::MAX,
        }
    }
}

impl WaveChannel {
    fn clock_length(&mut self) {
        if self.len_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn step_timer(&mut self, wave_ram: &[u8; 16]) {
        self.ticks_since_read = self.ticks_since_read.saturating_add(1);
        if !self.enabled {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.wave_pos = (self.wave_pos + 1) & 31;
            self.sample_buffer = wave_ram[(self.wave_pos >> 1) as usize];
            self.ticks_since_read = 0;
            self.timer = (2048 - self.freq) * 2;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }
        let nibble = if (self.wave_pos & 1) == 0 {
            self.sample_buffer >> 4
        } else {
            self.sample_buffer & 0x0F
        };
        let shift = match self.volume_code & 3 {
            0 => 4,
            1 => 0,
            2 => 1,
            _ => 2,
        };
        nibble >> shift
    }
}

#[derive(Clone, Default)]
struct NoiseChannel {
    enabled: bool,
    length_counter: u16,
    len_enabled: bool,
    env: Envelope,
    clock_shift: u8,
    width_mode: bool,
    divisor_code: u8,
    timer: u32,
    lfsr: u16,
}

impl NoiseChannel {
    fn period(&self) -> u32 {
        let base: u32 = match self.divisor_code & 7 {
            0 => 8,
            n => (n as u32) * 16,
        };
        base << self.clock_shift
    }

    fn clock_length(&mut self) {
        if self.len_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn step_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period();
            let xor = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
            self.lfsr = (self.lfsr >> 1) | (xor << 14);
            if self.width_mode {
                self.lfsr = (self.lfsr & !(1 << 6)) | (xor << 6);
            }
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || !self.env.dac_enabled() {
            return 0;
        }
        if (self.lfsr & 1) == 0 {
            self.env.volume
        } else {
            0
        }
    }
}

#[derive(Clone)]
pub struct Apu {
    pub enabled: bool,
    pub is_cgb: bool,
    regs: [u8; 23],
    pub wave_ram: [u8; 16],
    frame_seq_step: u8,
    ch1: SquareChannel,
    sweep: Sweep,
    ch2: SquareChannel,
    ch3: WaveChannel,
    ch4: NoiseChannel,
    sample_phase: u64,
    pub audio_buffer: Vec<(i16, i16)>,
}

impl Apu {
    pub fn new() -> Self {
        let mut apu = Apu {
            enabled: true,
            is_cgb: false,
            regs: [0; 23],
            wave_ram: [0; 16],
            frame_seq_step: 7,
            ch1: SquareChannel::default(),
            sweep: Sweep::default(),
            ch2: SquareChannel::default(),
            ch3: WaveChannel::default(),
            ch4: NoiseChannel::default(),
            sample_phase: 0,
            audio_buffer: Vec::new(),
        };
        // Synthesized post-boot register state
        apu.regs[0x00] = 0x80; // NR10
        apu.regs[0x01] = 0xBF; // NR11
        apu.regs[0x02] = 0xF3; // NR12
        apu.regs[0x03] = 0xFF; // NR13
        apu.regs[0x04] = 0xBF; // NR14
        apu.regs[0x06] = 0x3F; // NR21
        apu.regs[0x08] = 0xFF; // NR23
        apu.regs[0x09] = 0xBF; // NR24
        apu.regs[0x0A] = 0x7F; // NR30
        apu.regs[0x0B] = 0xFF; // NR31
        apu.regs[0x0C] = 0x9F; // NR32
        apu.regs[0x0D] = 0xFF; // NR33
        apu.regs[0x0E] = 0xBF; // NR34
        apu.regs[0x10] = 0xFF; // NR41
        apu.regs[0x13] = 0xBF; // NR44
        apu.regs[0x14] = 0x77; // NR50
        apu.regs[0x15] = 0xF3; // NR51
        apu.ch1.env.write_reg(0xF3);
        apu
    }

    fn first_half_of_length_period(&self) -> bool {
        (self.frame_seq_step & 1) == 0
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF25 => {
                let idx = (addr - 0xFF10) as usize;
                self.regs[idx] | READ_MASKS[idx]
            }
            0xFF26 => {
                let mut status = if self.enabled { 0x80 } else { 0x00 } | 0x70;
                if self.ch1.enabled {
                    status |= 0x01;
                }
                if self.ch2.enabled {
                    status |= 0x02;
                }
                if self.ch3.enabled {
                    status |= 0x04;
                }
                if self.ch4.enabled {
                    status |= 0x08;
                }
                status
            }
            0xFF27..=0xFF2F => 0xFF,
            0xFF30..=0xFF3F => {
                if self.ch3.enabled {
                    if self.is_cgb || self.ch3.ticks_since_read < 2 {
                        self.wave_ram[(self.ch3.wave_pos >> 1) as usize]
                    } else {
                        0xFF
                    }
                } else {
                    self.wave_ram[(addr - 0xFF30) as usize]
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF10..=0xFF25 => {
                if !self.enabled {
                    // On DMG, length counter registers remain writable while powered off
                    // (without latching duty cycle bits into the readable register).
                    if !self.is_cgb {
                        match addr {
                            0xFF11 => self.ch1.length_counter = 64 - (val & 0x3F) as u16,
                            0xFF16 => self.ch2.length_counter = 64 - (val & 0x3F) as u16,
                            0xFF1B => self.ch3.length_counter = 256 - val as u16,
                            0xFF20 => self.ch4.length_counter = 64 - (val & 0x3F) as u16,
                            _ => {}
                        }
                    }
                    return;
                }
                let idx = (addr - 0xFF10) as usize;
                self.regs[idx] = val;
                match addr {
                    // Channel 1
                    0xFF10 => self.sweep.write_nr10(val, &mut self.ch1.enabled),
                    0xFF11 => {
                        self.ch1.duty = val >> 6;
                        self.ch1.length_counter = 64 - (val & 0x3F) as u16;
                    }
                    0xFF12 => {
                        self.ch1.env.write_reg(val);
                        if !self.ch1.env.dac_enabled() {
                            self.ch1.enabled = false;
                        }
                    }
                    0xFF13 => {
                        self.ch1.freq = (self.ch1.freq & 0x700) | val as u16;
                    }
                    0xFF14 => {
                        self.ch1.freq = (self.ch1.freq & 0x0FF) | (((val & 0x07) as u16) << 8);
                        let old_len_en = self.ch1.len_enabled;
                        let new_len_en = (val & 0x40) != 0;
                        let trigger = (val & 0x80) != 0;
                        let first_half = self.first_half_of_length_period();
                        self.ch1.len_enabled = new_len_en;
                        if !old_len_en && new_len_en && first_half && self.ch1.length_counter > 0 {
                            self.ch1.length_counter -= 1;
                            if self.ch1.length_counter == 0 && !trigger {
                                self.ch1.enabled = false;
                            }
                        }
                        if trigger {
                            self.ch1.enabled = self.ch1.env.dac_enabled();
                            if self.ch1.length_counter == 0 {
                                self.ch1.length_counter = 64;
                                if new_len_en && first_half {
                                    self.ch1.length_counter -= 1;
                                }
                            }
                            self.ch1.timer = (2048 - self.ch1.freq) * 4;
                            self.ch1.env.trigger();
                            self.sweep.trigger(self.ch1.freq, &mut self.ch1.enabled);
                        }
                    }

                    // Channel 2
                    0xFF16 => {
                        self.ch2.duty = val >> 6;
                        self.ch2.length_counter = 64 - (val & 0x3F) as u16;
                    }
                    0xFF17 => {
                        self.ch2.env.write_reg(val);
                        if !self.ch2.env.dac_enabled() {
                            self.ch2.enabled = false;
                        }
                    }
                    0xFF18 => {
                        self.ch2.freq = (self.ch2.freq & 0x700) | val as u16;
                    }
                    0xFF19 => {
                        self.ch2.freq = (self.ch2.freq & 0x0FF) | (((val & 0x07) as u16) << 8);
                        let old_len_en = self.ch2.len_enabled;
                        let new_len_en = (val & 0x40) != 0;
                        let trigger = (val & 0x80) != 0;
                        let first_half = self.first_half_of_length_period();
                        self.ch2.len_enabled = new_len_en;
                        if !old_len_en && new_len_en && first_half && self.ch2.length_counter > 0 {
                            self.ch2.length_counter -= 1;
                            if self.ch2.length_counter == 0 && !trigger {
                                self.ch2.enabled = false;
                            }
                        }
                        if trigger {
                            self.ch2.enabled = self.ch2.env.dac_enabled();
                            if self.ch2.length_counter == 0 {
                                self.ch2.length_counter = 64;
                                if new_len_en && first_half {
                                    self.ch2.length_counter -= 1;
                                }
                            }
                            self.ch2.timer = (2048 - self.ch2.freq) * 4;
                            self.ch2.env.trigger();
                        }
                    }

                    // Channel 3 (Wave)
                    0xFF1A => {
                        self.ch3.dac_enabled = (val & 0x80) != 0;
                        if !self.ch3.dac_enabled {
                            self.ch3.enabled = false;
                        }
                    }
                    0xFF1B => {
                        self.ch3.length_counter = 256 - val as u16;
                    }
                    0xFF1C => {
                        self.ch3.volume_code = (val >> 5) & 0x03;
                    }
                    0xFF1D => {
                        self.ch3.freq = (self.ch3.freq & 0x700) | val as u16;
                    }
                    0xFF1E => {
                        self.ch3.freq = (self.ch3.freq & 0x0FF) | (((val & 0x07) as u16) << 8);
                        let old_len_en = self.ch3.len_enabled;
                        let new_len_en = (val & 0x40) != 0;
                        let trigger = (val & 0x80) != 0;
                        let first_half = self.first_half_of_length_period();
                        self.ch3.len_enabled = new_len_en;
                        if !old_len_en && new_len_en && first_half && self.ch3.length_counter > 0 {
                            self.ch3.length_counter -= 1;
                            if self.ch3.length_counter == 0 && !trigger {
                                self.ch3.enabled = false;
                            }
                        }
                        if trigger {
                            // DMG Wave RAM corruption when retriggering 2 T-cycles before a sample read
                            if !self.is_cgb && self.ch3.enabled && self.ch3.timer == 2 {
                                let idx = (((self.ch3.wave_pos + 1) & 31) >> 1) as usize;
                                if idx < 4 {
                                    self.wave_ram[0] = self.wave_ram[idx];
                                } else {
                                    let base = idx & !3;
                                    self.wave_ram.copy_within(base..base + 4, 0);
                                }
                            }
                            self.ch3.enabled = self.ch3.dac_enabled;
                            if self.ch3.length_counter == 0 {
                                self.ch3.length_counter = 256;
                                if new_len_en && first_half {
                                    self.ch3.length_counter -= 1;
                                }
                            }
                            self.ch3.timer = (2048 - self.ch3.freq) * 2 + 6;
                            self.ch3.wave_pos = 0;
                        }
                    }

                    // Channel 4 (Noise)
                    0xFF20 => {
                        self.ch4.length_counter = 64 - (val & 0x3F) as u16;
                    }
                    0xFF21 => {
                        self.ch4.env.write_reg(val);
                        if !self.ch4.env.dac_enabled() {
                            self.ch4.enabled = false;
                        }
                    }
                    0xFF22 => {
                        self.ch4.clock_shift = val >> 4;
                        self.ch4.width_mode = (val & 0x08) != 0;
                        self.ch4.divisor_code = val & 0x07;
                    }
                    0xFF23 => {
                        let old_len_en = self.ch4.len_enabled;
                        let new_len_en = (val & 0x40) != 0;
                        let trigger = (val & 0x80) != 0;
                        let first_half = self.first_half_of_length_period();
                        self.ch4.len_enabled = new_len_en;
                        if !old_len_en && new_len_en && first_half && self.ch4.length_counter > 0 {
                            self.ch4.length_counter -= 1;
                            if self.ch4.length_counter == 0 && !trigger {
                                self.ch4.enabled = false;
                            }
                        }
                        if trigger {
                            self.ch4.enabled = self.ch4.env.dac_enabled();
                            if self.ch4.length_counter == 0 {
                                self.ch4.length_counter = 64;
                                if new_len_en && first_half {
                                    self.ch4.length_counter -= 1;
                                }
                            }
                            self.ch4.timer = self.ch4.period();
                            self.ch4.lfsr = 0x7FFF;
                            self.ch4.env.trigger();
                        }
                    }
                    _ => {}
                }
            }
            0xFF26 => {
                let new_enabled = (val & 0x80) != 0;
                if self.enabled && !new_enabled {
                    self.power_off();
                } else if !self.enabled && new_enabled {
                    self.enabled = true;
                    self.frame_seq_step = 7;
                    if self.is_cgb {
                        self.ch1.length_counter = 0;
                        self.ch2.length_counter = 0;
                        self.ch3.length_counter = 0;
                        self.ch4.length_counter = 0;
                    }
                }
            }
            0xFF30..=0xFF3F => {
                if self.ch3.enabled {
                    if self.is_cgb || self.ch3.ticks_since_read < 2 {
                        self.wave_ram[(self.ch3.wave_pos >> 1) as usize] = val;
                    }
                } else {
                    self.wave_ram[(addr - 0xFF30) as usize] = val;
                }
            }
            _ => {}
        }
    }

    fn power_off(&mut self) {
        self.enabled = false;
        self.regs = [0; 23];
        self.frame_seq_step = 0;

        let len1 = if self.is_cgb { 0 } else { self.ch1.length_counter };
        let len2 = if self.is_cgb { 0 } else { self.ch2.length_counter };
        let len3 = if self.is_cgb { 0 } else { self.ch3.length_counter };
        let len4 = if self.is_cgb { 0 } else { self.ch4.length_counter };

        self.ch1 = SquareChannel {
            length_counter: len1,
            ..Default::default()
        };
        self.sweep.clear();
        self.ch2 = SquareChannel {
            length_counter: len2,
            ..Default::default()
        };
        self.ch3 = WaveChannel {
            length_counter: len3,
            ..Default::default()
        };
        self.ch4 = NoiseChannel {
            length_counter: len4,
            ..Default::default()
        };
    }

    /// Called on falling edge of DIV-APU bit (bit 12 of `div_counter` in 1x speed).
    pub fn clock_frame_sequencer(&mut self) {
        if !self.enabled {
            return;
        }
        self.frame_seq_step = (self.frame_seq_step + 1) & 7;
        match self.frame_seq_step {
            0 | 4 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
            }
            2 | 6 => {
                self.ch1.clock_length();
                self.ch2.clock_length();
                self.ch3.clock_length();
                self.ch4.clock_length();
                if let Some(new_freq) = self.sweep.clock(&mut self.ch1.enabled) {
                    self.ch1.freq = new_freq;
                    self.regs[0x03] = (new_freq & 0xFF) as u8;
                    self.regs[0x04] = (self.regs[0x04] & 0xF8) | ((new_freq >> 8) as u8 & 0x07);
                }
            }
            7 => {
                self.ch1.env.clock();
                self.ch2.env.clock();
                self.ch4.env.clock();
            }
            _ => {}
        }
    }

    /// Steps one T-cycle in the base (4.194 MHz) clock domain.
    pub fn tick(&mut self) {
        if self.enabled {
            self.ch1.step_timer();
            self.ch2.step_timer();
            self.ch3.step_timer(&self.wave_ram);
            self.ch4.step_timer();
        }
        self.sample_phase += SAMPLE_RATE as u64;
        if self.sample_phase >= CLOCK_HZ {
            self.sample_phase -= CLOCK_HZ;
            if self.audio_buffer.len() < MAX_BUFFERED_SAMPLES {
                self.audio_buffer.push(self.sample_stereo());
            }
        }
    }

    fn sample_stereo(&self) -> (i16, i16) {
        if !self.enabled {
            return (0, 0);
        }
        let s1 = self.ch1.output() as i32;
        let s2 = self.ch2.output() as i32;
        let s3 = self.ch3.output() as i32;
        let s4 = self.ch4.output() as i32;

        let nr51 = self.regs[0x15];
        let nr50 = self.regs[0x14];

        let mut left = 0i32;
        let mut right = 0i32;
        if nr51 & 0x10 != 0 {
            left += s1;
        }
        if nr51 & 0x20 != 0 {
            left += s2;
        }
        if nr51 & 0x40 != 0 {
            left += s3;
        }
        if nr51 & 0x80 != 0 {
            left += s4;
        }
        if nr51 & 0x01 != 0 {
            right += s1;
        }
        if nr51 & 0x02 != 0 {
            right += s2;
        }
        if nr51 & 0x04 != 0 {
            right += s3;
        }
        if nr51 & 0x08 != 0 {
            right += s4;
        }

        let left_vol = ((nr50 >> 4) & 0x07) as i32 + 1;
        let right_vol = (nr50 & 0x07) as i32 + 1;

        let l = (left * left_vol * 64).clamp(-32768, 32767) as i16;
        let r = (right * right_vol * 64).clamp(-32768, 32767) as i16;
        (l, r)
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}
