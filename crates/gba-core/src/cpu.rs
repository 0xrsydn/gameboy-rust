use std::{error::Error, fmt};

use crate::memory::{Memory, MemoryError};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

// Semantic tests compare architectural state after successful instructions.
// Error/rollback tests keep full Cpu equality, including buffered fetch results.
#[cfg(test)]
macro_rules! assert_cpu_arch_eq {
    ($actual:expr, $expected:expr $(, $($message:tt)+)?) => {{
        let mut actual = $actual.clone();
        let mut expected = $expected.clone();
        actual.invalidate_pipeline();
        expected.invalidate_pipeline();
        assert_eq!(actual, expected $(, $($message)+)?);
    }};
}

mod alu;
mod arm;
mod exception;
mod multiply;
mod pipeline;
mod status;
mod thumb;
mod timing;

pub use exception::Exception;
pub use status::Mode;
mod transfer;

#[cfg(test)]
mod arm_pipeline_tests;
#[cfg(test)]
mod bios_access_tests;
#[cfg(test)]
mod bios_write_tests;
#[cfg(test)]
mod compare_psr_tests;
#[cfg(test)]
mod dma_resume_tests;
#[cfg(test)]
mod exception_tests;
#[cfg(test)]
mod fetch_boundary_tests;
#[cfg(test)]
mod fetch_history_tests;
#[cfg(test)]
mod fetch_sequence_tests;
#[cfg(test)]
mod fetch_timing_tests;
#[cfg(test)]
mod instruction_tests;
#[cfg(test)]
mod iwram_dma_tests;
#[cfg(test)]
mod iwram_open_bus_tests;
#[cfg(test)]
mod iwram_refill_tests;
#[cfg(test)]
mod load_alias_tests;
#[cfg(test)]
mod open_bus_tests;
#[cfg(test)]
mod refill_timing_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thumb_empty_tests;
#[cfg(test)]
mod thumb_open_bus_tests;
#[cfg(test)]
mod thumb_pipeline_tests;
#[cfg(test)]
mod thumb_tests;
#[cfg(test)]
mod timing_tests;
#[cfg(test)]
mod transfer_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuError {
    Memory(MemoryError),
    UnsupportedInstruction { address: u32, instruction: u32 },
    UnsupportedThumbInstruction { address: u32, instruction: u16 },
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(error) => write!(f, "{error}"),
            Self::UnsupportedThumbInstruction {
                address,
                instruction,
            } => {
                write!(
                    f,
                    "unsupported Thumb instruction {instruction:#06x} at {address:#010x}"
                )
            }
            Self::UnsupportedInstruction {
                address,
                instruction,
            } => {
                write!(
                    f,
                    "unsupported instruction {instruction:#010x} at {address:#010x}"
                )
            }
        }
    }
}

impl Error for CpuError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            Self::UnsupportedInstruction { .. } | Self::UnsupportedThumbInstruction { .. } => None,
        }
    }
}

impl From<MemoryError> for CpuError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

/// Arithmetic status flags from the current program status register.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub negative: bool,
    pub zero: bool,
    /// For subtraction, carry means no unsigned borrow occurred.
    pub carry: bool,
    pub overflow: bool,
}

impl Flags {
    fn condition_passed(self, condition: u32) -> bool {
        match condition {
            0x0 => self.zero,                                    // EQ
            0x1 => !self.zero,                                   // NE
            0x2 => self.carry,                                   // CS/HS
            0x3 => !self.carry,                                  // CC/LO
            0x4 => self.negative,                                // MI
            0x5 => !self.negative,                               // PL
            0x6 => self.overflow,                                // VS
            0x7 => !self.overflow,                               // VC
            0x8 => self.carry && !self.zero,                     // HI
            0x9 => !self.carry || self.zero,                     // LS
            0xa => self.negative == self.overflow,               // GE
            0xb => self.negative != self.overflow,               // LT
            0xc => !self.zero && self.negative == self.overflow, // GT
            0xd => self.zero || self.negative != self.overflow,  // LE
            0xe => true,                                         // AL
            _ => false, // Reserved condition; rejected by Cpu::step.
        }
    }
}

/// Instruction encoding selected by the processor's T bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionSet {
    Arm,
    Thumb,
}

impl InstructionSet {
    pub(crate) fn width(self) -> u32 {
        match self {
            Self::Arm => 4,
            Self::Thumb => 2,
        }
    }

    fn align(self, address: u32) -> u32 {
        address & !(self.width() - 1)
    }
}

/// ARM/Thumb interpreter with register banks, instruction buffering, and nominal costs.
/// Clone/equality include buffered fetch results, instruction state, and next fetch kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    registers: [u32; 16],
    flags: Flags,
    instruction_set: InstructionSet,
    mode: Mode,
    irq_disabled: bool,
    fiq_disabled: bool,
    banks: status::Banks,
    pipeline: Option<pipeline::Pipeline>,
}

impl Cpu {
    /// Start directly at ARM-state test code in System mode, with interrupts enabled.
    /// This does not perform a GBA reset.
    pub fn new(entry_point: u32) -> Self {
        let mut registers = [0; 16];
        registers[15] = entry_point;
        Self {
            registers,
            flags: Flags::default(),
            instruction_set: InstructionSet::Arm,
            mode: Mode::System,
            irq_disabled: false,
            fiq_disabled: false,
            banks: status::Banks::default(),
            pipeline: None,
        }
    }

    /// r15 stores the next instruction address, not the pipeline-adjusted value.
    pub fn registers(&self) -> &[u32; 16] {
        &self.registers
    }

    pub fn pc(&self) -> u32 {
        self.registers[15]
    }

    pub fn flags(&self) -> Flags {
        self.flags
    }

    pub fn instruction_set(&self) -> InstructionSet {
        self.instruction_set
    }

    /// Execute one ARM instruction or one 16-bit Thumb instruction.
    /// Each half of a Thumb BL is a separate step.
    /// This CPU-only API does not execute DMA, advance clocks, or sample device interrupts.
    /// Enabled opcode prefetch still tracks nominal bus time, as in step_timed.
    /// It records HALTCNT writes but does not enforce HALT. Use Machine::step for integration.
    /// Unsupported operations and memory errors leave CPU state unchanged.
    /// These errors are development diagnostics, not emulated CPU exceptions.
    pub fn step(&mut self, memory: &mut Memory) -> Result<(), CpuError> {
        // Prefetch timing state must agree across CPU-only and timed calls.
        // Compute and discard nominal costs without advancing any device clocks.
        if memory.waitcnt() & 0x4000 != 0 {
            return self.step_timed(memory).map(|_| ());
        }
        let instruction = self.fetch(memory)?;
        self.execute_fetched(instruction, memory)
    }

    fn execute_fetched(
        &mut self,
        fetched: pipeline::Fetched,
        memory: &mut Memory,
    ) -> Result<(), CpuError> {
        let pipeline::Fetched {
            instruction,
            mut continuation,
            lookahead,
            fill,
        } = fetched;
        // Record the executing instruction, not an operand's pipelined PC value.
        // Always clear the access context, including on diagnostic errors.
        let pc = self.pc();
        let instruction_set = self.instruction_set;
        let summary = self.instruction_summary(instruction);
        memory.begin_cpu_access(pc, self.instruction_set, fill.as_ref(), &lookahead);
        let result = self.execute_instruction(instruction, memory);
        memory.end_cpu_access(result.is_ok());
        if result.is_ok() {
            memory.record_internal_cycles(summary.internal);
            if summary.refill {
                self.refill_pipeline(memory);
            } else {
                continuation.next_access = summary.next_fetch_kind();
                // A state change without an ordinary refill starts a cold sequence.
                self.pipeline = if self.instruction_set == instruction_set
                    && self.pc() == pc.wrapping_add(instruction_set.width())
                {
                    Some(continuation)
                } else {
                    None
                };
            }
        }
        result
    }

    fn execute_instruction(
        &mut self,
        instruction: u32,
        memory: &mut Memory,
    ) -> Result<(), CpuError> {
        if self.instruction_set == InstructionSet::Thumb {
            return self.execute_thumb(instruction as u16, memory);
        }
        let condition = instruction >> 28;
        // Reserved in ARMv4. Do not decode ARMv5 unconditional extensions.
        if condition == 0xf {
            return Err(self.unsupported(instruction));
        }
        if !self.flags.condition_passed(condition) {
            self.registers[15] = self.pc().wrapping_add(4);
            return Ok(());
        }
        self.execute_arm(instruction, memory)
    }

    fn unsupported(&self, instruction: u32) -> CpuError {
        match self.instruction_set {
            InstructionSet::Arm => CpuError::UnsupportedInstruction {
                address: self.pc(),
                instruction,
            },
            InstructionSet::Thumb => CpuError::UnsupportedThumbInstruction {
                address: self.pc(),
                instruction: instruction as u16,
            },
        }
    }

    fn branch_exchange(&mut self, target: u32) {
        self.instruction_set = if target & 1 == 0 {
            InstructionSet::Arm
        } else {
            InstructionSet::Thumb
        };
        self.registers[15] = self.instruction_set.align(target);
    }

    fn operand_register(&self, register: usize, pc_offset: u32) -> u32 {
        if register == 15 {
            self.pc().wrapping_add(pc_offset)
        } else {
            self.registers[register]
        }
    }
}

fn save_instruction_set(instruction_set: InstructionSet, writer: &mut Writer) {
    writer.u8(match instruction_set {
        InstructionSet::Arm => 0,
        InstructionSet::Thumb => 1,
    });
}

fn load_instruction_set(reader: &mut Reader<'_>) -> Result<InstructionSet, SnapshotError> {
    match reader.u8()? {
        0 => Ok(InstructionSet::Arm),
        1 => Ok(InstructionSet::Thumb),
        _ => Err(SnapshotError::Corrupt("instruction set")),
    }
}

fn save_mode(mode: Mode, writer: &mut Writer) {
    writer.u8(match mode {
        Mode::User => 0,
        Mode::Fiq => 1,
        Mode::Irq => 2,
        Mode::Supervisor => 3,
        Mode::Abort => 4,
        Mode::Undefined => 5,
        Mode::System => 6,
    });
}

fn load_mode(reader: &mut Reader<'_>) -> Result<Mode, SnapshotError> {
    match reader.u8()? {
        0 => Ok(Mode::User),
        1 => Ok(Mode::Fiq),
        2 => Ok(Mode::Irq),
        3 => Ok(Mode::Supervisor),
        4 => Ok(Mode::Abort),
        5 => Ok(Mode::Undefined),
        6 => Ok(Mode::System),
        _ => Err(SnapshotError::Corrupt("processor mode")),
    }
}

impl Snapshot for Cpu {
    fn save(&self, writer: &mut Writer) {
        let Self {
            registers,
            flags,
            instruction_set,
            mode,
            irq_disabled,
            fiq_disabled,
            banks,
            pipeline,
        } = self;
        writer.tag(b"CPU0");
        for register in registers {
            writer.u32(*register);
        }
        let Flags {
            negative,
            zero,
            carry,
            overflow,
        } = flags;
        writer.bool(*negative);
        writer.bool(*zero);
        writer.bool(*carry);
        writer.bool(*overflow);
        save_instruction_set(*instruction_set, writer);
        save_mode(*mode, writer);
        writer.bool(*irq_disabled);
        writer.bool(*fiq_disabled);
        banks.save_snapshot(writer);
        writer.option(pipeline.as_ref(), |pipeline, writer| {
            pipeline.save_snapshot(writer);
        });
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            registers,
            flags,
            instruction_set,
            mode,
            irq_disabled,
            fiq_disabled,
            banks,
            pipeline,
        } = self;
        reader.tag(b"CPU0")?;
        for register in registers {
            *register = reader.u32()?;
        }
        let Flags {
            negative,
            zero,
            carry,
            overflow,
        } = flags;
        *negative = reader.bool()?;
        *zero = reader.bool()?;
        *carry = reader.bool()?;
        *overflow = reader.bool()?;
        *instruction_set = load_instruction_set(reader)?;
        *mode = load_mode(reader)?;
        *irq_disabled = reader.bool()?;
        *fiq_disabled = reader.bool()?;
        banks.load_snapshot(reader)?;
        *pipeline = reader.option(|reader| pipeline::Pipeline::load_snapshot(reader))?;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::memory::ROM_START;

    fn populated_cpu() -> Cpu {
        const THUMB_STORE: [u8; 2] = [0x08, 0x60];
        let mut cpu = Cpu::new(ROM_START);
        cpu.instruction_set = InstructionSet::Thumb;
        cpu.registers[0] = 0xaabb_ccdd;
        cpu.registers[1] = 0x0200_0000;
        let mut memory = Memory::new(THUMB_STORE.repeat(8)).unwrap();
        cpu.step(&mut memory).unwrap();

        for (index, register) in cpu.registers[..15].iter_mut().enumerate() {
            *register = 0x1000 + index as u32;
        }
        cpu.registers[15] = ROM_START + 2;
        cpu.flags = Flags {
            negative: true,
            zero: false,
            carry: true,
            overflow: true,
        };
        cpu.irq_disabled = true;
        cpu.fiq_disabled = true;

        for (mode, seed) in [
            (Mode::User, 0x10_000),
            (Mode::Fiq, 0x20_000),
            (Mode::Irq, 0x30_000),
            (Mode::Supervisor, 0x40_000),
            (Mode::Abort, 0x50_000),
            (Mode::Undefined, 0x60_000),
            (Mode::System, 0x70_000),
        ] {
            cpu.switch_mode(mode);
            for (index, register) in cpu.registers[8..15].iter_mut().enumerate() {
                *register = seed + index as u32;
            }
            if !matches!(mode, Mode::User | Mode::System) {
                cpu.set_spsr(seed + 0x800);
            }
        }
        cpu.switch_mode(Mode::Irq);
        cpu
    }

    fn save(cpu: &Cpu) -> Vec<u8> {
        let mut writer = Writer::new();
        cpu.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn cpu_round_trips_register_banks_flags_and_pipeline() {
        let source = populated_cpu();
        let bytes = save(&source);
        let mut restored = Cpu::new(0);
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
    }

    #[test]
    fn every_strict_cpu_prefix_is_rejected() {
        let bytes = save(&populated_cpu());
        for end in 0..bytes.len() {
            let mut restored = Cpu::new(0);
            assert!(
                restored.load(&mut Reader::new(&bytes[..end])).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn cpu_rejects_unknown_instruction_set_discriminant() {
        let mut bytes = save(&populated_cpu());
        bytes[72] = 0xff;
        let mut restored = Cpu::new(0);
        assert!(matches!(
            restored.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt(_))
        ));
    }
}
