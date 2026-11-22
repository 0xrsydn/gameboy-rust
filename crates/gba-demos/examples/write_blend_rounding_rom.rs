//! Write the original brightness-rounding test ROM and print what each emulator predicts.
//! See docs/research/blend-rounding.md.
use std::{error::Error, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        return Err("usage: write_blend_rounding_rom OUTPUT.gba".into());
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&gba_demos::blend_rounding::blend_rounding_rom())?;
    println!("Wrote {path:?}. Run gbafix on a copy before flashing it to a cartridge.\n");
    print!("{}", gba_demos::blend_rounding::blend_rounding_table());
    Ok(())
}
