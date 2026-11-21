//! Embed the firmware the benchmark requires at address 0. GBA Eval's BIOS stub is MIT
//! licensed but not original to this repository, so it is never committed: point
//! `GBA_EVAL_BIOS` at a copy (`tools/prepare_gba_eval.py` fetches and verifies one). Without
//! it, the build embeds nothing and the library falls back to this emulator's firmware.

use std::{env, fs, path::PathBuf};

const BIOS_SIZE: usize = 16 * 1024;

fn main() {
    println!("cargo:rerun-if-env-changed=GBA_EVAL_BIOS");
    let bytes = match env::var_os("GBA_EVAL_BIOS") {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
            let bytes =
                fs::read(&path).unwrap_or_else(|error| panic!("GBA_EVAL_BIOS {path:?}: {error}"));
            assert_eq!(
                bytes.len(),
                BIOS_SIZE,
                "GBA_EVAL_BIOS {path:?} must be a 16 KiB BIOS image"
            );
            bytes
        }
        None => Vec::new(),
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out.join("bios.bin"), bytes).expect("write the embedded BIOS");
}
