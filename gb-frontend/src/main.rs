use gb_core::GameBoy;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "run" {
        eprintln!("usage: gb run <rom> [--serial] [--max-mcycles N]");
        std::process::exit(2);
    }
    let rom = std::fs::read(&args[2]).expect("cannot read rom");
    let max: u64 = args
        .iter()
        .position(|a| a == "--max-mcycles")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.parse().unwrap())
        .unwrap_or(30_000_000);
    let mut gb = GameBoy::new(&rom);
    let mut reported = 0usize;
    for _ in 0..max {
        gb.step();
        let out = gb.serial_output();
        if out.len() > reported {
            print!("{}", String::from_utf8_lossy(&out[reported..]));
            reported = out.len();
        }
    }
}
