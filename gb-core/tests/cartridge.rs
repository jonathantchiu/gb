use gb_core::GameBoy;

fn rom_with(cart_type: u8, ram_size_code: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = cart_type;
    rom[0x149] = ram_size_code;
    rom[0x100] = 0x18;
    rom[0x101] = 0xFE;
    rom
}

#[test]
fn battery_ram_round_trips_through_save_data() {
    let mut gb = GameBoy::new(&rom_with(0x03, 0x02));
    assert!(gb.has_battery());
    gb.bus.poke8(0x0000, 0x0A);
    gb.bus.poke8(0xA000, 0x42);
    gb.bus.poke8(0xBFFF, 0x99);
    let save = gb.save_data();
    assert_eq!(save.len(), 0x2000);

    let mut fresh = GameBoy::new(&rom_with(0x03, 0x02));
    fresh.load_save_data(&save);
    fresh.bus.poke8(0x0000, 0x0A);
    assert_eq!(fresh.bus.peek8(0xA000), 0x42);
    assert_eq!(fresh.bus.peek8(0xBFFF), 0x99);
}

#[test]
fn ram_is_unreadable_until_enabled() {
    let mut gb = GameBoy::new(&rom_with(0x03, 0x02));
    gb.bus.poke8(0xA000, 0x42);
    assert_eq!(gb.bus.peek8(0xA000), 0xFF);
    gb.bus.poke8(0x0000, 0x0A);
    assert_eq!(gb.bus.peek8(0xA000), 0x00);
}

#[test]
fn no_battery_means_no_save_data() {
    let gb = GameBoy::new(&rom_with(0x01, 0x00));
    assert!(!gb.has_battery());
    assert!(gb.save_data().is_empty());
}

#[test]
fn mbc3_rtc_counts_emulated_seconds_and_persists() {
    let mut gb = GameBoy::new(&rom_with(0x10, 0x03));
    gb.bus.poke8(0x0000, 0x0A);
    gb.bus.poke8(0x4000, 0x08);
    for _ in 0..3 {
        for _ in 0..(4_194_304 / 4) {
            gb.bus.tick_mcycle();
        }
    }
    gb.bus.poke8(0x6000, 0x00);
    gb.bus.poke8(0x6000, 0x01);
    assert_eq!(gb.bus.peek8(0xA000) & 0x3F, 3);

    let save = gb.save_data();
    assert_eq!(save.len(), 0x8000 + 10);
    let mut fresh = GameBoy::new(&rom_with(0x10, 0x03));
    fresh.load_save_data(&save);
    fresh.bus.poke8(0x0000, 0x0A);
    fresh.bus.poke8(0x4000, 0x08);
    fresh.bus.poke8(0x6000, 0x00);
    fresh.bus.poke8(0x6000, 0x01);
    assert_eq!(fresh.bus.peek8(0xA000) & 0x3F, 3);
}

#[test]
fn snapshot_restores_identical_execution() {
    let rom = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../gb-tests/roms/dmg-acid2/dmg-acid2.gb"
    ))
    .unwrap();
    let mut gb = GameBoy::new(&rom);
    for _ in 0..3 {
        gb.run_frame();
    }
    let snap = gb.clone();
    for _ in 0..5 {
        gb.run_frame();
    }
    let mut restored = snap;
    for _ in 0..5 {
        restored.run_frame();
    }
    assert_eq!(gb.bus.mcycles, restored.bus.mcycles);
    assert_eq!(gb.framebuffer(), restored.framebuffer());
    assert_eq!(gb.cpu.pc, restored.cpu.pc);
}

#[test]
fn clock_domains_timer_runs_twice_ppu_rate_in_double_speed() {
    let mut gb = GameBoy::new(&rom_with(0x00, 0x00));
    gb.bus.double_speed = true;
    // 1024 fast cycles = 512 slow cycles. Timer period at TAC rate 01 (bit 3,
    // ÷16) overflows TIMA after 256 fast increments of the selected bit.
    gb.bus.poke8(0xFF06, 0x00); // TMA
    gb.bus.poke8(0xFF05, 0xFE); // TIMA
    gb.bus.poke8(0xFF07, 0x05); // TAC: enable, rate 01
    for _ in 0..256 {
        gb.bus.tick_system_cycles(2);
    }
    assert!(gb.bus.if_reg & 0x04 != 0, "timer IRQ must fire");
    // PPU saw only 1024 * 1/2 = 512 dots: still on line 1 (2 lines = 912 dots).
    assert!(gb.bus.ppu.ly < 2, "ly={}", gb.bus.ppu.ly);
}

#[test]
fn clock_domains_match_single_rate_in_single_speed() {
    // Grayscale framebuffer after N M-cycles must be identical whether
    // double_speed is false or the caller ticks 2 fast cycles per slow one.
    let rom = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../gb-tests/roms/dmg-acid2/dmg-acid2.gb"
    ))
    .unwrap();
    // Same number of fast-domain steps; the PPU must advance at half rate.
    let mut a = GameBoy::new(&rom);
    for _ in 0..4096 {
        a.bus.tick_system_cycles(2);
    }
    let mut b = GameBoy::new(&rom);
    b.bus.double_speed = true;
    for _ in 0..4096 {
        b.bus.tick_system_cycles(2);
    }
    // Fast domain (timer/DIV) advanced identically in both machines...
    assert_eq!(a.bus.timer.div_counter(), b.bus.timer.div_counter());
    // ...while the slow domain (PPU) advanced half as far in double speed.
    assert!(b.bus.ppu.ly < a.bus.ppu.ly, "ly a={} b={}", a.bus.ppu.ly, b.bus.ppu.ly);
    // In single speed, tick_system_cycles(4) is exactly one M-cycle.
    let mut single = GameBoy::new(&rom);
    for _ in 0..1000 {
        single.bus.tick_mcycle();
    }
    let mut via2 = GameBoy::new(&rom);
    for _ in 0..1000 {
        via2.bus.tick_system_cycles(4);
    }
    assert_eq!(single.bus.timer.div_counter(), via2.bus.timer.div_counter());
    assert_eq!(single.framebuffer(), via2.framebuffer());
}

#[test]
fn apu_generates_samples_in_slow_clock_domain() {
    let mut gb = GameBoy::new(&rom_with(0x00, 0x00));
    gb.bus.poke8(0xFF26, 0x80); // NR52 power on
    gb.bus.poke8(0xFF24, 0x77); // NR50 master vol
    gb.bus.poke8(0xFF25, 0x11); // NR51 Ch1 L+R
    gb.bus.poke8(0xFF11, 0x80); // NR11 50% duty
    gb.bus.poke8(0xFF12, 0xF0); // NR12 vol=15
    gb.bus.poke8(0xFF13, 0x00); // NR13 freq lo
    gb.bus.poke8(0xFF14, 0x87); // NR14 trigger, freq=0x700

    for _ in 0..4096 {
        gb.bus.tick_system_cycles(4);
    }
    let single_samples = gb.take_audio_samples();
    assert!(!single_samples.is_empty());
    assert!(single_samples.iter().any(|&(l, r)| l > 0 && r > 0));

    let mut gb2 = GameBoy::new(&rom_with(0x00, 0x00));
    gb2.bus.double_speed = true;
    for _ in 0..8192 {
        gb2.bus.tick_system_cycles(4);
    }
    let double_samples = gb2.take_audio_samples();
    // 8192 fast M-cycles in double speed = 4096 slow M-cycles, so identical
    // sample count to 4096 single-speed M-cycles.
    assert_eq!(double_samples.len(), single_samples.len());
}

