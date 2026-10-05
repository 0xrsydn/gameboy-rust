//! Export original input-test instructions, never game or firmware content.
use std::{
    error::Error,
    fs::OpenOptions,
    io::{self, Write},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().filter(|path| !path.is_empty()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: write_rom_demo OUTPUT_PATH",
        )
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: write_rom_demo OUTPUT_PATH",
        )
        .into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&gba_demos::input_rom())?;
    file.flush()?;
    println!("Wrote original input-test ROM to {path:?}. Blue: idle; Z: red; Right: green.");
    Ok(())
}
