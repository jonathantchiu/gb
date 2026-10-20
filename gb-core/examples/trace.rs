use gb_core::GameBoy;
use std::collections::{HashMap, VecDeque};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Snap {
    pc: u16,
    a: u8,
    f: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    sp: u16,
    serial_len: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom = std::fs::read(&args[1]).unwrap();
    let max: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000_000);
    let mut gb = GameBoy::new(&rom);
    if args.get(3).map(|s| s.as_str()) == Some("memdump") {
        let at: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap();
        let addr: u16 = u16::from_str_radix(args.get(5).unwrap(), 16).unwrap();
        for _ in 0..at {
            gb.step();
        }
        for i in 0..16u16 {
            print!("{:#04x} ", gb.bus.peek8(addr + i * 16));
            for j in 1..16u16 {
                print!("{:02x} ", gb.bus.peek8(addr + i * 16 + j));
            }
            println!();
        }
        return;
    }
    if args.get(3).map(|s| s.as_str()) == Some("window") {
        let lo: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        for i in 0..max {
            let c = &gb.cpu;
            let pc = c.pc;
            if i >= lo {
                let op = gb.bus.peek8(pc);
                println!(
                    "{i:6} pc={pc:#06x} op={op:#04x} a={:#04x} f={:#04x} bc={:#06x} de={:#06x} hl={:#06x} sp={:#06x}",
                    c.a, c.f,
                    (c.b as u16) << 8 | c.c as u16,
                    (c.d as u16) << 8 | c.e as u16,
                    (c.h as u16) << 8 | c.l as u16,
                    c.sp
                );
            }
            gb.step();
        }
        return;
    }
    if args.get(3).map(|s| s.as_str()) == Some("progress") {
        for i in 0..max {
            if i % 500_000 == 0 {
                let c = &gb.cpu;
                println!(
                    "{i:9} pc={:#06x} sp={:#06x} halted={} ie={:#04x} if={:#04x} ly={} serial_len={}",
                    c.pc, c.sp, c.halted, gb.bus.ie, gb.bus.if_reg, gb.bus.ppu.ly,
                    gb.serial_output().len()
                );
            }
            gb.step();
        }
        println!("serial: {:?}", String::from_utf8_lossy(gb.serial_output()));
        return;
    }
    if args.get(3).map(|s| s.as_str()) == Some("resetwatch") {
        let mut last_pc = 0u16;
        let mut hist: VecDeque<(u16, u8)> = VecDeque::new();
        for i in 0..max {
            let pc = gb.cpu.pc;
            if pc < 0x100 && last_pc >= 0x100 {
                println!(
                    "step {i}: jump into vector region: {last_pc:#06x} -> {pc:#06x} (op at last: {:#04x})",
                    gb.bus.peek8(last_pc)
                );
            }
            if pc == 0x100 && last_pc != 0x100 {
                println!("step {i}: re-entered 0x100 from {last_pc:#06x}");
            }
            if (0xC300..0xE000).contains(&pc) && !(0xC300..0xE000).contains(&last_pc) {
                println!("step {i}: entered WRAM-D code at {pc:#06x} from {last_pc:#06x}; context:");
                for (p, o) in &hist {
                    println!("   pc={p:#06x} op={o:#04x}");
                }
                return;
            }
            hist.push_back((pc, gb.bus.peek8(pc)));
            if hist.len() > 24 {
                hist.pop_front();
            }
            last_pc = pc;
            gb.step();
        }
        println!("done {max} steps; serial={:?}", String::from_utf8_lossy(gb.serial_output()));
        return;
    }
    if let Some(n) = args.get(3).and_then(|s| s.parse::<u64>().ok()) {
        for i in 0..n {
            let c = &gb.cpu;
            let pc = c.pc;
            let op = gb.bus.peek8(pc);
            println!(
                "{i:6} pc={pc:#06x} op={op:#04x} a={:#04x} f={:#04x} bc={:#06x} de={:#06x} hl={:#06x} sp={:#06x}",
                c.a,
                c.f,
                (c.b as u16) << 8 | c.c as u16,
                (c.d as u16) << 8 | c.e as u16,
                (c.h as u16) << 8 | c.l as u16,
                c.sp
            );
            gb.step();
        }
        return;
    }
    let mut seen: HashMap<Snap, u64> = HashMap::new();
    let mut printed = 0usize;
    let mut trace: Vec<u16> = Vec::new();
    for i in 0..max {
        let c = &gb.cpu;
        let snap = Snap {
            pc: c.pc,
            a: c.a,
            f: c.f,
            b: c.b,
            c: c.c,
            d: c.d,
            e: c.e,
            h: c.h,
            l: c.l,
            sp: c.sp,
            serial_len: gb.serial_output().len(),
        };
        if let Some(&prev) = seen.get(&snap) {
            println!("state repeated: step {prev} == step {i} (period {})", i - prev);
            let period = (i - prev) as usize;
            let cycle_start = trace.len() - period;
            for j in cycle_start..trace.len() {
                let pc = trace[j];
                let op = gb.bus.peek8(pc);
                let n1 = gb.bus.peek8(pc.wrapping_add(1));
                let n2 = gb.bus.peek8(pc.wrapping_add(2));
                println!("pc={pc:#06x} op={op:#04x} {n1:#04x} {n2:#04x}");
            }
            return;
        }
        seen.insert(snap, i);
        trace.push(c.pc);
        gb.step();
        let out = gb.serial_output();
        if out.len() > printed {
            print!("{}", String::from_utf8_lossy(&out[printed..]));
            printed = out.len();
        }
    }
    println!("no loop found in {max} steps");
}
