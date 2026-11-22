//! Run ROMs as a player would and report any that stop on a core diagnostic.
//!
//! Each ROM starts from this emulator's own firmware through `gba_emu::Session` (the code the
//! GBA Eval build runs) and plays FRAMES frames while tapping Start and A and holding the
//! d-pad now and then. A real GBA never stops, so every stop is a compatibility gap to fix.
//! Exits 1 when any ROM stops. See docs/testing.md.
//!
//!     cargo run --release -p gba-eval --example survey -- 3600 roms/*.gba

use std::{process::ExitCode, time::Instant};

use gba_emu::Session;

/// Start, then A, every two seconds, with Right and Up held in between.
fn keys(frame: u64) -> u16 {
    match frame % 120 {
        0..=5 => 0x08,
        30..=50 => 0x10,
        60..=65 => 0x01,
        90..=110 => 0x40,
        _ => 0,
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(frames) = args.next().and_then(|text| text.parse::<u64>().ok()) else {
        eprintln!("usage: survey FRAMES ROM...");
        return ExitCode::FAILURE;
    };
    let mut stopped = 0;
    let mut total = 0;
    for path in args {
        total += 1;
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let rom = match std::fs::read(&path) {
            Ok(rom) => rom,
            Err(error) => {
                println!("{name:32} unreadable: {error}");
                stopped += 1;
                continue;
            }
        };
        let mut session = Session::new(gba_core::bios::image());
        if let Err(error) = session.load(rom) {
            println!("{name:32} cannot start: {error}");
            stopped += 1;
            continue;
        }
        let started = Instant::now();
        let mut report = None;
        for frame in 0..frames {
            session.set_keys(keys(frame));
            session.run_frame();
            if let Some(error) = session.error() {
                report = Some(format!("STOPPED at frame {frame}: {error}"));
                break;
            }
        }
        match report {
            Some(report) => {
                stopped += 1;
                println!("{name:32} {report}");
            }
            None => println!(
                "{name:32} ok, {frames} frames in {:.1} s",
                started.elapsed().as_secs_f64()
            ),
        }
    }
    println!("{} of {total} ran without stopping", total - stopped);
    if stopped == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
