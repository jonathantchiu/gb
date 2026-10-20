use gb_core::{Button, GameBoy, SCREEN_H, SCREEN_W};
use minifb::{Key, Scale, Window, WindowOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const USAGE: &str = "\
usage:
  gb <rom.gb>                         open a window and play
  gb headless <rom.gb> [--mcycles N]  run without a window, print serial output

keys: arrows = d-pad, Z = A, X = B, Enter = Start, RShift/Backspace = Select,
      Tab = fast-forward, P = pause, F5 = save state, F7 = load state, Esc = quit";

const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 * 70224 / 4_194_304);

const KEYMAP: &[(Key, Button)] = &[
    (Key::Right, Button::Right),
    (Key::Left, Button::Left),
    (Key::Up, Button::Up),
    (Key::Down, Button::Down),
    (Key::Z, Button::A),
    (Key::X, Button::B),
    (Key::Enter, Button::Start),
    (Key::RightShift, Button::Select),
    (Key::Backspace, Button::Select),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("headless") => headless(&args[1..]),
        Some(rom) if !rom.starts_with('-') => {
            if let Err(e) = play(Path::new(rom)) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn headless(args: &[String]) {
    let Some(rom_path) = args.first() else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let rom = std::fs::read(rom_path).expect("cannot read rom");
    let boot = std::env::var_os("GB_BOOT_ROM").map(|p| std::fs::read(p).expect("GB_BOOT_ROM"));
    let max: u64 = args
        .iter()
        .position(|a| a == "--mcycles")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.parse().expect("--mcycles N"))
        .unwrap_or(30_000_000);
    let mut gb = GameBoy::with_boot_rom(&rom, boot);
    let mut reported = 0usize;
    while gb.bus.mcycles < max {
        gb.step();
        let out = gb.serial_output();
        if out.len() > reported {
            print!("{}", String::from_utf8_lossy(&out[reported..]));
            reported = out.len();
        }
    }
}

fn save_path(rom: &Path) -> PathBuf {
    rom.with_extension("sav")
}

fn play(rom_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let rom = std::fs::read(rom_path)?;
    let boot = std::env::var_os("GB_BOOT_ROM").map(|p| std::fs::read(p).expect("GB_BOOT_ROM"));
    let mut gb = GameBoy::with_boot_rom(&rom, boot);

    let sav = save_path(rom_path);
    if gb.has_battery() {
        if let Ok(data) = std::fs::read(&sav) {
            gb.load_save_data(&data);
        }
    }

    let title = format!("gb - {}", gb.title());
    let mut window = Window::new(
        &title,
        SCREEN_W,
        SCREEN_H,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(0);

    let mut pixels = vec![0u32; SCREEN_W * SCREEN_H];
    let mut paused = false;
    let mut save_state: Option<GameBoy> = None;
    let mut next_frame = Instant::now();
    let mut last_save = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::P, minifb::KeyRepeat::No) {
            paused = !paused;
        }
        if window.is_key_pressed(Key::F5, minifb::KeyRepeat::No) {
            save_state = Some(gb.clone());
        }
        if window.is_key_pressed(Key::F7, minifb::KeyRepeat::No) {
            if let Some(state) = &save_state {
                gb = state.clone();
            }
        }
        for &(key, button) in KEYMAP {
            gb.set_button(button, window.is_key_down(key));
        }

        let fast_forward = window.is_key_down(Key::Tab);
        if !paused {
            let frames = if fast_forward { 8 } else { 1 };
            for _ in 0..frames {
                gb.run_frame();
            }
        }

        for (dst, &shade) in pixels.iter_mut().zip(gb.framebuffer()) {
            let s = shade as u32;
            *dst = (s << 16) | (s << 8) | s;
        }
        window.update_with_buffer(&pixels, SCREEN_W, SCREEN_H)?;

        if gb.has_battery() && last_save.elapsed() > Duration::from_secs(2) {
            let _ = std::fs::write(&sav, gb.save_data());
            last_save = Instant::now();
        }

        if fast_forward || paused {
            next_frame = Instant::now();
        } else {
            next_frame += FRAME_TIME;
            let now = Instant::now();
            if next_frame > now {
                std::thread::sleep(next_frame - now);
            } else if now - next_frame > FRAME_TIME * 4 {
                next_frame = now;
            }
        }
    }

    if gb.has_battery() {
        std::fs::write(&sav, gb.save_data())?;
    }
    Ok(())
}
