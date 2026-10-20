use gb_tests::{
    load_ratchet, roms_dir, run_acid2_rom, run_mooneye_rom, run_serial_rom, Status,
    DMG_ACID2_EXPECTED_HASH, MAX_MCYCLES,
};

fn check_serial(rom_rel: &str) {
    let ratchet = load_ratchet();
    let entry = ratchet
        .iter()
        .find(|e| e.rom == rom_rel)
        .unwrap_or_else(|| panic!("{rom_rel} not in ratchet.txt"));
    let path = roms_dir().join(rom_rel);
    let max = match entry.expected {
        Status::Pass => MAX_MCYCLES,
        Status::Fail => 5_000_000,
    };
    let result = run_serial_rom(&path, max);
    assert_eq!(
        result.status, entry.expected,
        "{rom_rel}: ratchet={:?} actual={:?}\nserial output:\n{}",
        entry.expected, result.status, result.serial
    );
}

fn check_mooneye(rom_rel: &str) {
    let ratchet = load_ratchet();
    let entry = ratchet
        .iter()
        .find(|e| e.rom == rom_rel)
        .unwrap_or_else(|| panic!("{rom_rel} not in ratchet.txt"));
    let path = roms_dir().join(rom_rel);
    let result = run_mooneye_rom(&path, 5_000_000);
    assert_eq!(
        result.status, entry.expected,
        "{rom_rel}: ratchet={:?} actual={:?}\n{}",
        entry.expected, result.status, result.serial
    );
}

fn check_acid2(rom_rel: &str, expected_hash: u64) {
    let ratchet = load_ratchet();
    let entry = ratchet
        .iter()
        .find(|e| e.rom == rom_rel)
        .unwrap_or_else(|| panic!("{rom_rel} not in ratchet.txt"));
    let path = roms_dir().join(rom_rel);
    let result = run_acid2_rom(&path, expected_hash, 12);
    assert_eq!(
        result.status, entry.expected,
        "{rom_rel}: ratchet={:?} actual={:?}\n{}",
        entry.expected, result.status, result.serial
    );
}

macro_rules! serial_rom_tests {
    ($($name:ident => $path:literal,)*) => {
        $(#[test]
        fn $name() {
            check_serial($path);
        })*
    };
}

macro_rules! mooneye_rom_tests {
    ($($name:ident => $path:literal,)*) => {
        $(#[test]
        fn $name() {
            check_mooneye($path);
        })*
    };
}

serial_rom_tests! {
    cpu_instrs_01_special => "cpu_instrs/individual/01-special.gb",
    cpu_instrs_02_interrupts => "cpu_instrs/individual/02-interrupts.gb",
    cpu_instrs_03_op_sp_hl => "cpu_instrs/individual/03-op sp,hl.gb",
    cpu_instrs_04_op_r_imm => "cpu_instrs/individual/04-op r,imm.gb",
    cpu_instrs_05_op_rp => "cpu_instrs/individual/05-op rp.gb",
    cpu_instrs_06_ld_r_r => "cpu_instrs/individual/06-ld r,r.gb",
    cpu_instrs_07_jr_jp_call_ret_rst => "cpu_instrs/individual/07-jr,jp,call,ret,rst.gb",
    cpu_instrs_08_misc_instrs => "cpu_instrs/individual/08-misc instrs.gb",
    cpu_instrs_09_op_r_r => "cpu_instrs/individual/09-op r,r.gb",
    cpu_instrs_10_bit_ops => "cpu_instrs/individual/10-bit ops.gb",
    cpu_instrs_11_op_a_hl => "cpu_instrs/individual/11-op a,(hl).gb",
    instr_timing => "instr_timing/instr_timing.gb",
    mem_timing_01 => "mem_timing/individual/01-read_timing.gb",
    mem_timing_02 => "mem_timing/individual/02-write_timing.gb",
    mem_timing_03 => "mem_timing/individual/03-modify_timing.gb",
    mem_timing2_01 => "mem_timing-2/rom_singles/01-read_timing.gb",
    mem_timing2_02 => "mem_timing-2/rom_singles/02-write_timing.gb",
    mem_timing2_03 => "mem_timing-2/rom_singles/03-modify_timing.gb",
    blargg_halt_bug => "halt_bug.gb",
}

mooneye_rom_tests! {
    mooneye_timer_div_write => "mooneye/acceptance/timer/div_write.gb",
    mooneye_timer_rapid_toggle => "mooneye/acceptance/timer/rapid_toggle.gb",
    mooneye_timer_tim00 => "mooneye/acceptance/timer/tim00.gb",
    mooneye_timer_tim00_div_trigger => "mooneye/acceptance/timer/tim00_div_trigger.gb",
    mooneye_timer_tim01 => "mooneye/acceptance/timer/tim01.gb",
    mooneye_timer_tim01_div_trigger => "mooneye/acceptance/timer/tim01_div_trigger.gb",
    mooneye_timer_tim10 => "mooneye/acceptance/timer/tim10.gb",
    mooneye_timer_tim10_div_trigger => "mooneye/acceptance/timer/tim10_div_trigger.gb",
    mooneye_timer_tim11 => "mooneye/acceptance/timer/tim11.gb",
    mooneye_timer_tim11_div_trigger => "mooneye/acceptance/timer/tim11_div_trigger.gb",
    mooneye_timer_tima_reload => "mooneye/acceptance/timer/tima_reload.gb",
    mooneye_timer_tima_write_reloading => "mooneye/acceptance/timer/tima_write_reloading.gb",
    mooneye_timer_tma_write_reloading => "mooneye/acceptance/timer/tma_write_reloading.gb",
    mooneye_add_sp_e_timing => "mooneye/acceptance/add_sp_e_timing.gb",
    mooneye_call_cc_timing => "mooneye/acceptance/call_cc_timing.gb",
    mooneye_call_cc_timing2 => "mooneye/acceptance/call_cc_timing2.gb",
    mooneye_call_timing => "mooneye/acceptance/call_timing.gb",
    mooneye_call_timing2 => "mooneye/acceptance/call_timing2.gb",
    mooneye_div_timing => "mooneye/acceptance/div_timing.gb",
    mooneye_ei_sequence => "mooneye/acceptance/ei_sequence.gb",
    mooneye_ei_timing => "mooneye/acceptance/ei_timing.gb",
    mooneye_halt_ime0_ei => "mooneye/acceptance/halt_ime0_ei.gb",
    mooneye_halt_ime0_nointr_timing => "mooneye/acceptance/halt_ime0_nointr_timing.gb",
    mooneye_halt_ime1_timing => "mooneye/acceptance/halt_ime1_timing.gb",
    mooneye_if_ie_registers => "mooneye/acceptance/if_ie_registers.gb",
    mooneye_instr_daa => "mooneye/acceptance/instr/daa.gb",
    mooneye_interrupts_ie_push => "mooneye/acceptance/interrupts/ie_push.gb",
    mooneye_intr_timing => "mooneye/acceptance/intr_timing.gb",
    mooneye_jp_cc_timing => "mooneye/acceptance/jp_cc_timing.gb",
    mooneye_jp_timing => "mooneye/acceptance/jp_timing.gb",
    mooneye_ld_hl_sp_e_timing => "mooneye/acceptance/ld_hl_sp_e_timing.gb",
    mooneye_oam_dma_basic => "mooneye/acceptance/oam_dma/basic.gb",
    mooneye_oam_dma_reg_read => "mooneye/acceptance/oam_dma/reg_read.gb",
    mooneye_oam_dma_sources => "mooneye/acceptance/oam_dma/sources-GS.gb",
    mooneye_oam_dma_restart => "mooneye/acceptance/oam_dma_restart.gb",
    mooneye_oam_dma_start => "mooneye/acceptance/oam_dma_start.gb",
    mooneye_oam_dma_timing => "mooneye/acceptance/oam_dma_timing.gb",
    mooneye_pop_timing => "mooneye/acceptance/pop_timing.gb",
    mooneye_push_timing => "mooneye/acceptance/push_timing.gb",
    mooneye_rapid_di_ei => "mooneye/acceptance/rapid_di_ei.gb",
    mooneye_ret_cc_timing => "mooneye/acceptance/ret_cc_timing.gb",
    mooneye_ret_timing => "mooneye/acceptance/ret_timing.gb",
    mooneye_reti_intr_timing => "mooneye/acceptance/reti_intr_timing.gb",
    mooneye_reti_timing => "mooneye/acceptance/reti_timing.gb",
    mooneye_rst_timing => "mooneye/acceptance/rst_timing.gb",
    mooneye_bits_mem_oam => "mooneye/acceptance/bits/mem_oam.gb",
    mooneye_bits_reg_f => "mooneye/acceptance/bits/reg_f.gb",
}

#[test]
fn dmg_acid2() {
    check_acid2("dmg-acid2/dmg-acid2.gb", DMG_ACID2_EXPECTED_HASH);
}
