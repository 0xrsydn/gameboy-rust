//! Block transfers shared by ARM and Thumb, plus ARM swaps.
//! Data aborts and per-access device event timing are not modeled.

use super::{Cpu, CpuError, InstructionSet};
use crate::memory::Memory;

impl Cpu {
    pub(super) fn block_transfer(
        &mut self,
        instruction: u32,
        memory: &mut Memory,
    ) -> Result<(), CpuError> {
        let base_register = ((instruction >> 16) & 15) as usize;
        let write_back = instruction & (1 << 21) != 0;
        // A PC base reads PC+8, as in mGBA 0.10.5; writing it back stays diagnostic.
        if base_register == 15 && write_back {
            return Err(self.unsupported(instruction));
        }

        let encoded_list = instruction & 0xffff;
        // ARM7's empty list transfers PC, but calculates addresses/writeback
        // as if sixteen registers were selected.
        let (list, span) = if encoded_list == 0 {
            (1 << 15, 64)
        } else {
            (encoded_list, encoded_list.count_ones() * 4)
        };
        let pre = instruction & (1 << 24) != 0;
        let up = instruction & (1 << 23) != 0;
        let load = instruction & (1 << 20) != 0;
        let special = instruction & (1 << 22) != 0;
        let user_bank = special && !(load && list & (1 << 15) != 0);
        // As in mGBA 0.10.5, `^` in User mode names the registers already in use, an LDM
        // with PC restores SPSR only where one exists, and a user-bank STM writes back to
        // the current bank. An empty list with `^` and a user-bank LDM with writeback
        // stay diagnostic.
        if special && encoded_list == 0 || user_bank && load && write_back {
            return Err(self.unsupported(instruction));
        }
        let restore = if special && !user_bank && self.spsr().is_some() {
            Some(self.return_status(instruction)?)
        } else {
            None
        };
        let base = self.operand_register(base_register, 8);
        let adjusted = if up {
            base.wrapping_add(span)
        } else {
            base.wrapping_sub(span)
        };
        let start = match (up, pre) {
            (true, false) => base,                      // IA
            (true, true) => base.wrapping_add(4),       // IB
            (false, false) => adjusted.wrapping_add(4), // DA
            (false, true) => adjusted,                  // DB
        };
        // Unlike LDR, block loads align down without rotating the loaded word.
        // The low base bits stay on the address bus, where only the 8-bit save bus sees them,
        // and are retained in writeback.
        let mut address = start;
        let mut next = self.registers;
        next[15] = self.pc().wrapping_add(self.instruction_set.width());
        let base_in_list = list & (1 << base_register) != 0;
        if write_back && !(load && base_in_list) {
            next[base_register] = adjusted;
        }

        let mut writes = [(0, 0); 16];
        let mut write_count = 0;
        for (register, destination) in next.iter_mut().enumerate() {
            if list & (1 << register) == 0 {
                continue;
            }
            if load {
                // The 8-bit save bus repeats one byte in each loaded word.
                let value = memory.load_multiple(address)?;
                *destination = if register == 15 && restore.is_none() {
                    self.instruction_set.align(value)
                } else {
                    value
                };
            } else {
                let value = if write_back
                    && register == base_register
                    && register as u32 != list.trailing_zeros()
                {
                    // ARM7 stores the updated base when it is not first in Rlist.
                    adjusted
                } else if user_bank {
                    self.user_register(register)
                } else {
                    // Stores use one instruction beyond the ordinary operand PC.
                    // Thumb can store PC here only through an empty register list.
                    let pc_offset = if self.instruction_set == InstructionSet::Arm {
                        12
                    } else {
                        6
                    };
                    self.operand_register(register, pc_offset)
                };
                writes[write_count] = (address, value);
                write_count += 1;
            }
            address = address.wrapping_add(4);
        }
        if !load {
            memory.write_words(&writes[..write_count])?;
        }
        // Commit only after every access succeeds. This keeps diagnostics retryable.
        if user_bank && load {
            for (register, value) in next.into_iter().enumerate() {
                if list & (1 << register) != 0 {
                    self.set_user_register(register, value);
                }
            }
            self.registers[15] = next[15];
        } else {
            self.registers = next;
        }
        if let Some((status, mode)) = restore {
            // Save loaded banked registers and writeback in the outgoing bank.
            self.apply_status(status, mode);
            self.registers[15] = self.instruction_set.align(self.pc());
        }
        Ok(())
    }

    pub(super) fn swap(&mut self, instruction: u32, memory: &mut Memory) -> Result<(), CpuError> {
        let base_register = ((instruction >> 16) & 15) as usize;
        let destination = ((instruction >> 12) & 15) as usize;
        let source = (instruction & 15) as usize;
        // A PC base or source reads PC+8, as in mGBA 0.10.5. mGBA writes a swapped value
        // to PC without refilling its pipeline, so a PC destination stays diagnostic.
        if destination == 15 {
            return Err(self.unsupported(instruction));
        }
        // Snapshot operands so source/destination/base aliases use old values.
        let address = self.operand_register(base_register, 8);
        let value = self.operand_register(source, 8);
        let previous = if instruction & (1 << 22) != 0 {
            let previous = u32::from(memory.read8(address)?);
            memory.write8(address, value as u8)?;
            previous
        } else {
            let previous = memory.load32(address)?;
            memory.store32(address, value)?;
            previous
        };
        self.registers[destination] = previous;
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }
}
