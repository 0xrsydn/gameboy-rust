use std::{error::Error, fmt};

use crate::memory::{Memory, MemoryError};

mod alu;
mod arm;
mod exception;
mod status;
mod thumb;
mod timing;

pub use exception::Exception;
pub use status::Mode;
mod transfer;

#[cfg(test)]
mod bios_access_tests;
#[cfg(test)]
mod compare_psr_tests;
#[cfg(test)]
mod exception_tests;
#[cfg(test)]
mod instruction_tests;
#[cfg(test)]
mod load_alias_tests;
#[cfg(test)]
mod open_bus_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thumb_empty_tests;
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
    fn width(self) -> u32 {
        match self {
            Self::Arm => 4,
            Self::Thumb => 2,
        }
    }

    fn align(self, address: u32) -> u32 {
        address & !(self.width() - 1)
    }
}

/// ARM/Thumb interpreter with register banks and optional nominal cycle costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    registers: [u32; 16],
    flags: Flags,
    instruction_set: InstructionSet,
    mode: Mode,
    irq_disabled: bool,
    fiq_disabled: bool,
    banks: status::Banks,
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
    /// It records HALTCNT writes but does not enforce HALT. Use Machine::step for integration.
    /// Unsupported operations and memory errors leave CPU state unchanged.
    /// These errors are development diagnostics, not emulated CPU exceptions.
    pub fn step(&mut self, memory: &mut Memory) -> Result<(), CpuError> {
        let instruction = self.fetch(memory)?;
        self.execute_fetched(instruction, memory)
    }

    fn fetch(&self, memory: &Memory) -> Result<u32, CpuError> {
        Ok(match self.instruction_set {
            InstructionSet::Arm => memory.read32(self.pc())?,
            InstructionSet::Thumb => u32::from(memory.read16(self.pc())?),
        })
    }

    fn execute_fetched(&mut self, instruction: u32, memory: &mut Memory) -> Result<(), CpuError> {
        // Record the executing instruction, not an operand's pipelined PC value.
        // Always clear the access context, including on diagnostic errors.
        memory.begin_cpu_access(self.pc(), self.instruction_set);
        let result = self.execute_instruction(instruction, memory);
        memory.end_cpu_access(result.is_ok());
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
