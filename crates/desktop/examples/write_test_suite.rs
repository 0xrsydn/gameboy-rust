//! Export original ROM instructions and a reproducible smoke-test manifest.
//! The destination directory must not already exist. No Nintendo content is used.
use serde_json::json;
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

fn words(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
}
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()
}
fn main() -> Result<(), Box<dyn Error>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: write_test_suite NEW_DIRECTORY",
        )
    };
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(
        args.next()
            .filter(|path| !path.is_empty())
            .ok_or_else(invalid)?,
    );
    if args.next().is_some() {
        return Err(invalid().into());
    }
    fs::create_dir(&directory)?;
    write_new(
        &directory.join("arm.gba"),
        &words(&[
            0xe3a0_002a, // MOV r0,#42
            0xe3a0_1402, // MOV r1,#0x02000000
            0xe581_0000, // STR r0,[r1]
            0xeaff_fffe, // completion: B .
        ]),
    )?;
    let mut thumb = words(&[0xe59f_1000, 0xe12f_ff11, 0x0800_000d]); // LDR r1,literal; BX r1
    thumb.extend([0x2a, 0x20, 0xfe, 0xe7]); // Thumb MOV r0,#42; completion: B .
    write_new(&directory.join("thumb.gba"), &thumb)?;
    write_new(
        &directory.join("division.gba"),
        &words(&[
            0xe3a0_002a, // MOV r0,#42
            0xe3a0_1005, // MOV r1,#5
            0xef06_0000, // SWI Div (original BIOS replacement)
            0xeaff_fffe, // completion: B .
        ]),
    )?;
    let manifest = json!({"version":1,"cases":[
        {"name":"arm-store","rom":"arm.gba","step_limit":10000,
         "completion":{"pc":"0x0800000c","instruction_set":"arm"},
         "checks":[{"kind":"register","index":0,"equals":42},
                   {"kind":"memory32","address":"0x02000000","equals":42},
                   {"kind":"cpsr","equals":"0x1f"}]},
        {"name":"thumb-move","rom":"thumb.gba","step_limit":10000,
         "completion":{"pc":"0x0800000e","instruction_set":"thumb"},
         "checks":[{"kind":"register","index":0,"equals":42}]},
        {"name":"bios-division","rom":"division.gba","step_limit":10000,
         "completion":{"pc":"0x0800000c","instruction_set":"arm"},
         "checks":[{"kind":"register","index":0,"equals":8},
                   {"kind":"register","index":1,"equals":2},
                   {"kind":"register","index":3,"equals":8}]}
    ]});
    write_new(
        &directory.join("suite.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Wrote original ARM, Thumb, and BIOS test ROMs with suite.json in {directory:?}");
    Ok(())
}
