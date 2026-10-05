//! ARM7 processor modes, register banks, and status-register transfers.

use super::{Cpu, CpuError, Flags, InstructionSet};

/// ARMv4T processor mode. User and System share a register bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Mode {
    User = 0x10,
    Fiq = 0x11,
    Irq = 0x12,
    Supervisor = 0x13,
    Abort = 0x17,
    Undefined = 0x1b,
    System = 0x1f,
}

impl Mode {
    fn decode(status: u32) -> Option<Self> {
        Some(match status & 31 {
            0x10 => Self::User,
            0x11 => Self::Fiq,
            0x12 => Self::Irq,
            0x13 => Self::Supervisor,
            0x17 => Self::Abort,
            0x1b => Self::Undefined,
            0x1f => Self::System,
            _ => return None,
        })
    }

    fn bank(self) -> usize {
        match self {
            Self::User | Self::System => 0,
            Self::Fiq => 1,
            Self::Irq => 2,
            Self::Supervisor => 3,
            Self::Abort => 4,
            Self::Undefined => 5,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Banks {
    high: [[u32; 5]; 2], // r8-r12: shared and FIQ
    sp_lr: [[u32; 2]; 6],
    spsr: [u32; 5],
}

impl Cpu {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Current program status register. Reserved bits read as zero.
    pub fn cpsr(&self) -> u32 {
        (u32::from(self.flags.negative) << 31)
            | (u32::from(self.flags.zero) << 30)
            | (u32::from(self.flags.carry) << 29)
            | (u32::from(self.flags.overflow) << 28)
            | (u32::from(self.irq_disabled) << 7)
            | (u32::from(self.fiq_disabled) << 6)
            | (u32::from(self.instruction_set == InstructionSet::Thumb) << 5)
            | self.mode as u32
    }

    /// Saved program status register. User and System have no SPSR.
    pub fn spsr(&self) -> Option<u32> {
        self.mode
            .bank()
            .checked_sub(1)
            .map(|bank| self.banks.spsr[bank])
    }

    pub(super) fn set_spsr(&mut self, value: u32) {
        self.banks.spsr[self.mode.bank() - 1] = value;
    }

    pub(super) fn switch_mode(&mut self, mode: Mode) {
        // Save the active bank before loading the new one. User/System share it.
        self.banks.high[usize::from(self.mode == Mode::Fiq)]
            .copy_from_slice(&self.registers[8..13]);
        self.banks.sp_lr[self.mode.bank()].copy_from_slice(&self.registers[13..15]);
        self.registers[8..13].copy_from_slice(&self.banks.high[usize::from(mode == Mode::Fiq)]);
        self.registers[13..15].copy_from_slice(&self.banks.sp_lr[mode.bank()]);
        self.mode = mode;
    }

    // Callers validate the mode before committing any CPU or memory changes.
    pub(super) fn apply_status(&mut self, value: u32, mode: Mode) {
        self.switch_mode(mode);
        self.flags = Flags {
            negative: value & (1 << 31) != 0,
            zero: value & (1 << 30) != 0,
            carry: value & (1 << 29) != 0,
            overflow: value & (1 << 28) != 0,
        };
        self.irq_disabled = value & (1 << 7) != 0;
        self.fiq_disabled = value & (1 << 6) != 0;
        self.instruction_set = if value & (1 << 5) == 0 {
            InstructionSet::Arm
        } else {
            InstructionSet::Thumb
        };
    }

    pub(super) fn return_status(&self, instruction: u32) -> Result<(u32, Mode), CpuError> {
        let value = self.spsr().ok_or_else(|| self.unsupported(instruction))?;
        let mode = Mode::decode(value).ok_or_else(|| self.unsupported(instruction))?;
        Ok((value, mode))
    }

    pub(super) fn user_register(&self, register: usize) -> u32 {
        match register {
            8..=12 if self.mode == Mode::Fiq => self.banks.high[0][register - 8],
            13..=14 if self.mode.bank() != 0 => self.banks.sp_lr[0][register - 13],
            _ => self.operand_register(register, 12),
        }
    }

    pub(super) fn set_user_register(&mut self, register: usize, value: u32) {
        match register {
            8..=12 if self.mode == Mode::Fiq => self.banks.high[0][register - 8] = value,
            13..=14 if self.mode.bank() != 0 => self.banks.sp_lr[0][register - 13] = value,
            _ => self.registers[register] = value,
        }
    }

    pub(super) fn read_status(&mut self, instruction: u32) -> Result<(), CpuError> {
        let destination = ((instruction >> 12) & 15) as usize;
        if destination == 15 {
            return Err(self.unsupported(instruction));
        }
        let value = if instruction & (1 << 22) == 0 {
            self.cpsr()
        } else {
            self.spsr().ok_or_else(|| self.unsupported(instruction))?
        };
        self.registers[destination] = value;
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }

    pub(super) fn write_status(&mut self, instruction: u32) -> Result<(), CpuError> {
        let value = if instruction & (1 << 25) != 0 {
            (instruction & 255).rotate_right(((instruction >> 8) & 15) * 2)
        } else {
            let source = (instruction & 15) as usize;
            if source == 15 {
                return Err(self.unsupported(instruction));
            }
            self.registers[source]
        };
        let saved = instruction & (1 << 22) != 0;
        let previous = if saved {
            self.spsr().ok_or_else(|| self.unsupported(instruction))?
        } else {
            self.cpsr()
        };
        let mut mask = 0;
        if instruction & (1 << 19) != 0 {
            mask |= 0xf000_0000;
        }
        if instruction & (1 << 16) != 0 && (saved || self.mode != Mode::User) {
            mask |= 0xff;
        }
        let next = (previous & !mask) | (value & mask);
        if saved {
            // Invalid mode bits can be stored, but cannot be used for a return.
            self.set_spsr(next);
        } else {
            let mode = Mode::decode(next).ok_or_else(|| self.unsupported(instruction))?;
            // ARM7 forbids changing T through MSR CPSR. Diagnose that form.
            if (previous ^ next) & (1 << 5) != 0 {
                return Err(self.unsupported(instruction));
            }
            self.apply_status(next, mode);
        }
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }
}
