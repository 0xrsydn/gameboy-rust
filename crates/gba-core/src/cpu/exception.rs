//! Exception entry. Memory errors remain diagnostics rather than hardware aborts.

use super::{Cpu, InstructionSet, Mode};

/// ARM7 exception sources. GBA hardware does not expose every source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    UndefinedInstruction,
    SoftwareInterrupt,
    PrefetchAbort,
    DataAbort,
    Irq,
    Fiq,
}

impl Cpu {
    /// Start at the reset vector in ARM Supervisor mode, with IRQ/FIQ disabled.
    /// Registers and RAM are not initialized as a Nintendo BIOS would initialize them.
    pub fn at_reset() -> Self {
        let mut cpu = Self::new(0);
        cpu.mode = Mode::Supervisor;
        cpu.irq_disabled = true;
        cpu.fiq_disabled = true;
        cpu
    }

    /// Enter an exception explicitly, regardless of interrupt masks.
    /// For synchronous exceptions, PC must identify the faulting instruction.
    /// For IRQ/FIQ, PC must identify the next instruction, between CPU steps.
    /// This does not execute a BIOS service or fetch the vector instruction.
    pub fn enter_exception(&mut self, exception: Exception) {
        let (mode, vector, offset) = match exception {
            Exception::UndefinedInstruction => {
                (Mode::Undefined, 0x04, self.instruction_set.width())
            }
            Exception::SoftwareInterrupt => (Mode::Supervisor, 0x08, self.instruction_set.width()),
            Exception::PrefetchAbort => (Mode::Abort, 0x0c, 4),
            Exception::DataAbort => (Mode::Abort, 0x10, 8),
            Exception::Irq => (Mode::Irq, 0x18, 4),
            Exception::Fiq => (Mode::Fiq, 0x1c, 4),
        };
        let previous = self.cpsr();
        let link = self.pc().wrapping_add(offset);
        self.switch_mode(mode);
        self.set_spsr(previous);
        self.registers[14] = link;
        self.registers[15] = vector;
        self.instruction_set = InstructionSet::Arm;
        self.irq_disabled = true;
        if exception == Exception::Fiq {
            self.fiq_disabled = true;
        }
    }

    /// Sample external interrupt lines between instructions. FIQ has priority.
    /// Returns true when an unmasked interrupt was taken. Machine connects the GBA IRQ line.
    pub fn take_interrupt(&mut self, irq: bool, fiq: bool) -> bool {
        if fiq && !self.fiq_disabled {
            self.enter_exception(Exception::Fiq);
        } else if irq && !self.irq_disabled {
            self.enter_exception(Exception::Irq);
        } else {
            return false;
        }
        true
    }
}
