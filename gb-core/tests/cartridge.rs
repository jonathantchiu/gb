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
