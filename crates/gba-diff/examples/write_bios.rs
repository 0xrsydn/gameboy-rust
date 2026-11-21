//! Write this emulator's original 16 KiB firmware image to a file.
//!
//! Development-only tooling for references that run a BIOS image instead of a high-level
//! BIOS, such as `mesen-ref` driven by `tools/score_mgba_suite_reference.py --bios`.
//! (`gba-diff` itself passes the image to Mesen through a temporary file.)

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("usage: write_bios PATH");
        return ExitCode::from(2);
    };
    match fs::write(path, gba_core::bios::image()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot write {path}: {error}");
            ExitCode::from(1)
        }
    }
}
