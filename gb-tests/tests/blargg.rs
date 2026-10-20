use gb_tests::{load_ratchet, roms_dir, run_serial_rom, Status, MAX_MCYCLES};

fn check(rom_rel: &str) {
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

macro_rules! rom_tests {
    ($($name:ident => $path:literal,)*) => {
        $(#[test]
        fn $name() {
            check($path);
        })*
    };
}

rom_tests! {
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
}
