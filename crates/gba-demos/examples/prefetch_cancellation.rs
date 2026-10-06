//! Diagnostic only: a matching boundary total does not make a timer mismatch pass.
use std::process::ExitCode;

use gba_demos::prefetch_probe::{measure, ProbeError, PUBLISHED_READ_DELTAS, WAITCNT_SETTINGS};

fn run() -> Result<bool, ProbeError> {
    eprintln!("Original ARM read probe; published reference: PrefetchAbuse 9ca57c13.");
    eprintln!("Timer samples are unadjusted. This is not a run of the upstream ROM.");
    println!("idle_cycles,waitcnt,published_delta,boundary_delta,timer_delta,boundary_matches,timer_matches");
    let mut boundary_matches = 0;
    let mut timer_matches = 0;
    for (index, expected) in PUBLISHED_READ_DELTAS.into_iter().enumerate() {
        for (waitcnt, expected) in WAITCNT_SETTINGS.into_iter().zip(expected) {
            let control = measure(waitcnt, index as u8 + 1, false)?;
            let read = measure(waitcnt, index as u8 + 1, true)?;
            let boundary_delta = read.boundary_cycles as i64 - control.boundary_cycles as i64;
            let timer_delta = i64::from(read.timer_sample) - i64::from(control.timer_sample);
            let boundary_ok = boundary_delta == i64::from(expected);
            let timer_ok = timer_delta == i64::from(expected);
            boundary_matches += usize::from(boundary_ok);
            timer_matches += usize::from(timer_ok);
            println!(
                "{},0x{waitcnt:04x},{expected},{boundary_delta},{timer_delta},{boundary_ok},{timer_ok}",
                index + 1
            );
        }
    }
    let cases = PUBLISHED_READ_DELTAS.len() * WAITCNT_SETTINGS.len();
    eprintln!(
        "Boundary matches: {boundary_matches}/{cases}; timer matches: {timer_matches}/{cases}."
    );
    eprintln!(
        "Full-buffer restart, Thumb, cartridge writes, and page boundaries are outside this probe."
    );
    Ok(boundary_matches == cases && timer_matches == cases)
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("prefetch_cancellation takes no arguments");
        return ExitCode::from(2);
    }
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
