fn main() {
    let rom = std::fs::read("gb-tests/roms/cpu_instrs/cpu_instrs.gb").unwrap();
    let mut gb = gb_core::GameBoy::new(&rom);
    let d = &rom;
    for bank in 1..=3u8 {
        gb.bus.poke8(0x2000, bank);
        let mut mism = 0;
        for off in 0x4000..0x8000usize {
            let want = d[bank as usize * 0x4000 + (off - 0x4000)];
            let got = gb.bus.peek8(off as u16);
            if want != got {
                mism += 1;
                if mism < 4 {
                    println!("bank {bank} off {off:#06x}: got {got:#04x} want {want:#04x}");
                }
            }
        }
        println!("bank {bank}: {mism} mismatches");
    }
}
