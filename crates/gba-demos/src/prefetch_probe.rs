//! Original ARM probe for ROM-read cancellation timing, not an upstream ROM port.
//! Published observations and scope: docs/research/prefetch-cancellation.md.
//! Timer samples remain unadjusted. End-of-instruction clock differences are a
//! separate diagnostic, not a replacement for a hardware-visible timer result.

use std::{error::Error, fmt};

use gba_core::{
    cpu::Cpu,
    io::{TIMER_BASE, WAITCNT},
    machine::{Machine, MachineError},
    memory::{Memory, MemoryError, ROM_START},
    timing::StepTiming,
};

pub const WAITCNT_SETTINGS: [u16; 4] = [0x4000, 0x4004, 0x4010, 0x4014];

/// Published hardware observations, indexed by idle count minus one, then WAITCNT_SETTINGS.
/// Source: zaydlang/PrefetchAbuse, revision 9ca57c13da7e3c569937f99a42e7c1caca029a2d,
/// src/main.c read_expected. These are measurement facts, not imported implementation code.
/// They measure a test-minus-control interval, not the isolated ROM data-access cost.
pub const PUBLISHED_READ_DELTAS: [[u16; 4]; 8] = [
    [0x11, 0x0f, 0x0f, 0x0d],
    [0x11, 0x0f, 0x0f, 0x0d],
    [0x11, 0x0f, 0x0f, 0x0d],
    [0x11, 0x0f, 0x10, 0x0e],
    [0x11, 0x0f, 0x0f, 0x0d],
    [0x11, 0x0f, 0x0f, 0x0d],
    [0x11, 0x0f, 0x10, 0x0e],
    [0x11, 0x0f, 0x0f, 0x0d],
];

#[derive(Debug)]
pub enum ProbeError {
    IdleCycles(u8),
    Memory(MemoryError),
    Machine(MachineError),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdleCycles(value) => write!(f, "idle cycles must be 1 through 8, got {value}"),
            Self::Memory(error) => write!(f, "cannot prepare prefetch probe: {error}"),
            Self::Machine(error) => write!(f, "cannot execute prefetch probe: {error}"),
        }
    }
}

impl Error for ProbeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdleCycles(_) => None,
            Self::Memory(error) => Some(error),
            Self::Machine(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    /// Actual timer value loaded by the ARM program, with no timing correction.
    pub timer_sample: u16,
    /// Clock interval between completion of timer enable and completion of timer sampling.
    pub boundary_cycles: u64,
    pub sample_timing: StepTiming,
    pub rom_read_timing: Option<StepTiming>,
    /// Sum of the multiply instructions' internal cycles, excluding other instructions.
    pub multiply_internal_cycles: u32,
    pub steps: usize,
    pub completion_pc: u32,
}

/// Execute one bounded original ARM sequence with or without the extra ROM read.
/// WAITCNT is host setup, outside the measured interval. No BIOS or assets are used.
/// Other WAITCNT settings are allowed for experiments but have no published oracle here.
pub fn measure(
    waitcnt: u16,
    idle_cycles: u8,
    with_rom_read: bool,
) -> Result<Measurement, ProbeError> {
    if !(1..=8).contains(&idle_cycles) {
        return Err(ProbeError::IdleCycles(idle_cycles));
    }
    const ENTRY: usize = 0x100;
    const LITERALS: usize = 0x300;
    let mut rom = 0xe1a0_0000_u32.to_le_bytes().repeat(256);
    let registers = [
        (0, TIMER_BASE),
        (1, 0x0080_0000), // Timer 0: zero reload, enable, prescaler 1, no IRQ.
        (2, ROM_START),
        (8, 0xaa),
        (9, 0xaaaa),
        (10, 0x00aa_aaaa),
        (11, 0xaaaa_aaaa),
    ];
    let mut code = Vec::new();
    for (index, (register, value)) in registers.into_iter().enumerate() {
        let offset = ENTRY + index * 4;
        let literal = LITERALS + index * 4;
        code.push(0xe59f_0000 | register << 12 | (literal - offset - 8) as u32);
        rom[literal..literal + 4].copy_from_slice(&value.to_le_bytes());
    }
    let enable_index = code.len();
    code.push(0xe580_1000); // STR r1,[r0]: start timer, then expose one free cartridge cycle.
    let multiply_start = code.len();
    let mut remaining = idle_cycles;
    while remaining != 0 {
        let cycles = remaining.min(4);
        let rs = u32::from(7 + cycles);
        code.push(0xe004_0098 | rs << 8); // MUL r4,r8,Rs: 1..4 internal cycles.
        remaining -= cycles;
    }
    let multiply_end = code.len();
    if with_rom_read {
        code.push(0xe592_6000); // LDR r6,[r2]: ROM data cancels opcode prefetch.
    }
    code.push(0xe1d0_50b0); // LDRH r5,[r0]: sample timer at the same instruction phase in each run.
    for (index, instruction) in code.iter().enumerate() {
        let offset = ENTRY + index * 4;
        rom[offset..offset + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    let mut memory = Memory::new(rom).map_err(ProbeError::Memory)?;
    memory
        .write16(WAITCNT, waitcnt)
        .map_err(ProbeError::Memory)?;
    let mut machine = Machine::new(Cpu::new(ROM_START + ENTRY as u32), memory);
    let mut start = 0;
    let mut multiply_internal_cycles = 0;
    let mut rom_read_timing = None;
    // The generated sequence is straight-line and bounded. Stop after the timer sample,
    // before any padding or literal is executed. Extra lookahead bytes remain mapped.
    for index in 0..code.len() {
        machine.step().map_err(ProbeError::Machine)?;
        if index == enable_index {
            start = machine.cycles();
        }
        if (multiply_start..multiply_end).contains(&index) {
            multiply_internal_cycles += machine.last_timing().internal_cycles;
        }
        if with_rom_read && index == multiply_end {
            rom_read_timing = Some(machine.last_timing());
        }
    }
    Ok(Measurement {
        timer_sample: machine.cpu().registers()[5] as u16,
        boundary_cycles: machine.cycles() - start,
        sample_timing: machine.last_timing(),
        rom_read_timing,
        multiply_internal_cycles,
        steps: code.len(),
        completion_pc: machine.cpu().pc(),
    })
}
