//! Original code that configures a timer, handles one IRQ, and returns.
//! Uses nominal instruction/bus costs; device updates occur at instruction boundaries.

use crate::{
    cpu::Cpu,
    machine::Machine,
    memory::{Memory, MemoryError, BIOS_SIZE, ROM_START},
};

pub const TIMER_DEMO_STEPS: usize = 21;

pub fn timer_demo() -> Result<Machine, MemoryError> {
    let rom = [
        0xe3a0_0301_u32, // MOV r0, #0x04000000
        0xe280_0c01,     // ADD r0, r0, #0x100 (Timer 0)
        0xe3e0_100f,     // MVN r1, #15 (low half = 0xfff0)
        0xe1c0_10b0,     // STRH r1, [r0] (reload)
        0xe3a0_10c0,     // MOV r1, #0xc0 (enable + local IRQ)
        0xe3a0_2008,     // MOV r2, #8 (Timer 0 IRQ bit)
        0xe280_3c01,     // ADD r3, r0, #0x100 (IE)
        0xe1c3_20b0,     // STRH r2, [r3] (IE)
        0xe3a0_2001,     // MOV r2, #1
        0xe583_2008,     // STR r2, [r3, #8] (IME)
        0xe1c0_10b2,     // STRH r1, [r0, #2] (start timer)
        0xeaff_fffe,     // B . (waiting for IRQ, not hardware HALT)
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();

    // A test vector and handler, not a Nintendo BIOS. The handler deliberately
    // changes shared r2 and r10; it does not implement the GBA BIOS calling convention.
    let mut bios = vec![0; BIOS_SIZE];
    bios[0x18..0x1c].copy_from_slice(&0xea00_0010_u32.to_le_bytes()); // B 0x60
    for (index, instruction) in [
        0xe3a0_2000_u32, // MOV r2, #0
        0xe1c0_20b2,     // STRH r2, [r0, #2] (stop timer)
        0xe3a0_2008,     // MOV r2, #8
        0xe1c3_20b2,     // STRH r2, [r3, #2] (acknowledge IF)
        0xe28a_a001,     // ADD r10, r10, #1
        0xe25e_f004,     // SUBS pc, lr, #4 (return from IRQ)
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 0x60 + index * 4;
        bios[offset..offset + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    Ok(Machine::new(
        Cpu::new(ROM_START),
        Memory::with_bios(rom, bios)?,
    ))
}
