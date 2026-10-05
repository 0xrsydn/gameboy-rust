use super::{
    alu::{self, Shift},
    Cpu, CpuError,
};
use crate::memory::Memory;

impl Cpu {
    pub(super) fn execute_arm(
        &mut self,
        instruction: u32,
        memory: &mut Memory,
    ) -> Result<(), CpuError> {
        if instruction & 0x0f00_0000 == 0x0f00_0000 {
            self.enter_exception(super::Exception::SoftwareInterrupt);
            return Ok(());
        }
        if instruction & 0x0fbf_0fff == 0x010f_0000 {
            return self.read_status(instruction);
        }
        if instruction & 0x0fb0_fff0 == 0x0120_f000 || instruction & 0x0fb0_f000 == 0x0320_f000 {
            return self.write_status(instruction);
        }
        if instruction & 0x0fff_fff0 == 0x012f_ff10 {
            let target = self.operand_register((instruction & 15) as usize, 8);
            self.branch_exchange(target);
            return Ok(());
        }
        if instruction & 0x0e00_0000 == 0x0a00_0000 {
            let address = self.pc();
            let offset = (((instruction & 0x00ff_ffff) << 8) as i32 >> 6) as u32;
            if instruction & (1 << 24) != 0 {
                self.registers[14] = address.wrapping_add(4); // BL return address
            }
            self.registers[15] = address.wrapping_add(8).wrapping_add(offset);
            return Ok(());
        }
        if instruction & 0x0e00_0000 == 0x0800_0000 {
            return self.block_transfer(instruction, memory);
        }
        // SWP shares the 1001 low bits with multiply and halfword transfers.
        if instruction & 0x0fb0_0ff0 == 0x0100_0090 {
            return self.swap(instruction, memory);
        }
        if instruction & 0x0f80_00f0 == 0x0080_0090 {
            return self.multiply_long(instruction);
        }
        if instruction & 0x0fc0_00f0 == 0x0000_0090 {
            return self.multiply(instruction);
        }
        if instruction & 0x0e00_0090 == 0x0000_0090 {
            return self.halfword_transfer(instruction, memory);
        }
        if instruction & 0x0c00_0000 == 0x0400_0000 {
            return self.single_transfer(instruction, memory);
        }
        if instruction & 0x0c00_0000 == 0 {
            return self.data_processing(instruction);
        }
        Err(self.unsupported(instruction))
    }

    fn data_processing(&mut self, instruction: u32) -> Result<(), CpuError> {
        let opcode = (instruction >> 21) & 15;
        let set_flags = instruction & (1 << 20) != 0;
        let test = (8..=11).contains(&opcode);
        let source = ((instruction >> 16) & 15) as usize;
        let destination = ((instruction >> 12) & 15) as usize;
        // S=0 in the test/compare opcode range is PSR transfer or an extension.
        if test && !set_flags {
            return Err(self.unsupported(instruction));
        }
        let restore = if !test && destination == 15 && set_flags {
            Some(self.return_status(instruction)?)
        } else {
            None
        };
        let immediate = instruction & (1 << 25) != 0;
        let register_shift = !immediate && instruction & (1 << 4) != 0;
        let pc_offset = if register_shift { 12 } else { 8 };
        let (operand, carry) = if immediate {
            let rotation = ((instruction >> 8) & 15) * 2;
            let value = (instruction & 255).rotate_right(rotation);
            (
                value,
                if rotation == 0 {
                    self.flags.carry
                } else {
                    value >> 31 != 0
                },
            )
        } else {
            let amount = if register_shift {
                let shift_register = ((instruction >> 8) & 15) as usize;
                if instruction & (1 << 7) != 0 || shift_register == 15 {
                    return Err(self.unsupported(instruction));
                }
                self.registers[shift_register] & 255
            } else {
                (instruction >> 7) & 31
            };
            alu::shift(
                self.operand_register((instruction & 15) as usize, pc_offset),
                Shift::decode(instruction >> 5),
                amount,
                self.flags.carry,
                register_shift,
            )
        };
        let (result, flags) = alu::execute(
            opcode,
            self.operand_register(source, pc_offset),
            operand,
            carry,
            self.flags,
        );
        if let Some((status, mode)) = restore {
            self.apply_status(status, mode);
        } else if set_flags {
            self.flags = flags;
        }
        let next = if !test && destination == 15 {
            self.instruction_set.align(result)
        } else {
            self.pc().wrapping_add(4)
        };
        if !test && destination != 15 {
            self.registers[destination] = result;
        }
        self.registers[15] = next;
        Ok(())
    }

    fn single_transfer(&mut self, instruction: u32, memory: &mut Memory) -> Result<(), CpuError> {
        let base_register = ((instruction >> 16) & 15) as usize;
        let destination = ((instruction >> 12) & 15) as usize;
        let pre = instruction & (1 << 24) != 0;
        let write_bit = instruction & (1 << 21) != 0;
        let write_back = !pre || write_bit;
        let byte = instruction & (1 << 22) != 0;
        let load = instruction & (1 << 20) != 0;
        if (!pre && write_bit) // LDRT/STRT access semantics are not implemented.
            || (write_back && (base_register == 15 || (load && base_register == destination)))
            || (byte && destination == 15)
        {
            return Err(self.unsupported(instruction));
        }
        let offset = if instruction & (1 << 25) != 0 {
            let rm = (instruction & 15) as usize;
            if instruction & (1 << 4) != 0 || rm == 15 {
                return Err(self.unsupported(instruction));
            }
            alu::shift(
                self.registers[rm],
                Shift::decode(instruction >> 5),
                (instruction >> 7) & 31,
                self.flags.carry,
                false,
            )
            .0
        } else {
            instruction & 0xfff
        };
        let base = self.operand_register(base_register, 8);
        let adjusted = offset_address(base, offset, instruction);
        let address = if pre { adjusted } else { base };
        let next = self.pc().wrapping_add(4);
        // All validation precedes memory writes. CPU changes follow successful access.
        let value = if load {
            if byte {
                u32::from(memory.read8(address)?)
            } else {
                memory.read32(address & !3)?.rotate_right((address & 3) * 8)
            }
        } else {
            let value = self.operand_register(destination, 12);
            if byte {
                memory.write8(address, value as u8)?;
            } else {
                memory.write32(address & !3, value)?;
            }
            value
        };
        if write_back {
            self.registers[base_register] = adjusted;
        }
        if load && destination != 15 {
            self.registers[destination] = value;
        }
        // ARMv4 LDR PC does not switch to Thumb, even if bit zero is set.
        self.registers[15] = if load && destination == 15 {
            value & !3
        } else {
            next
        };
        Ok(())
    }

    fn halfword_transfer(&mut self, instruction: u32, memory: &mut Memory) -> Result<(), CpuError> {
        let kind = (instruction >> 5) & 3;
        let load = instruction & (1 << 20) != 0;
        let pre = instruction & (1 << 24) != 0;
        let write_bit = instruction & (1 << 21) != 0;
        let write_back = !pre || write_bit;
        let base_register = ((instruction >> 16) & 15) as usize;
        let destination = ((instruction >> 12) & 15) as usize;
        if kind == 0
            || (!load && kind != 1)
            || (!pre && write_bit)
            || destination == 15
            || (write_back && (base_register == 15 || (load && base_register == destination)))
        {
            return Err(self.unsupported(instruction));
        }
        let offset = if instruction & (1 << 22) != 0 {
            ((instruction >> 4) & 0xf0) | (instruction & 15)
        } else {
            let rm = (instruction & 15) as usize;
            if instruction & 0xf00 != 0 || rm == 15 {
                return Err(self.unsupported(instruction));
            }
            self.registers[rm]
        };
        let base = self.operand_register(base_register, 8);
        let adjusted = offset_address(base, offset, instruction);
        let address = if pre { adjusted } else { base };
        let value = if load {
            match kind {
                1 => u32::from(memory.read16(address & !1)?).rotate_right((address & 1) * 8),
                2 => memory.read8(address)? as i8 as i32 as u32,
                _ if address & 1 != 0 => memory.read8(address)? as i8 as i32 as u32,
                _ => memory.read16(address)? as i16 as i32 as u32,
            }
        } else {
            memory.write16(address & !1, self.registers[destination] as u16)?;
            0
        };
        if write_back {
            self.registers[base_register] = adjusted;
        }
        if load {
            self.registers[destination] = value;
        }
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }

    fn multiply(&mut self, instruction: u32) -> Result<(), CpuError> {
        let rd = ((instruction >> 16) & 15) as usize;
        let rn = ((instruction >> 12) & 15) as usize;
        let rs = ((instruction >> 8) & 15) as usize;
        let rm = (instruction & 15) as usize;
        let accumulate = instruction & (1 << 21) != 0;
        if [rd, rn, rs, rm].contains(&15) || rd == rm || (!accumulate && rn != 0) {
            return Err(self.unsupported(instruction));
        }
        let mut result = self.registers[rm].wrapping_mul(self.registers[rs]);
        if accumulate {
            result = result.wrapping_add(self.registers[rn]);
        }
        self.registers[rd] = result;
        if instruction & (1 << 20) != 0 {
            self.flags.negative = result >> 31 != 0;
            self.flags.zero = result == 0;
            // ARM7 carry is unspecified after multiply; preserve it deterministically.
        }
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }

    fn multiply_long(&mut self, instruction: u32) -> Result<(), CpuError> {
        let hi = ((instruction >> 16) & 15) as usize;
        let lo = ((instruction >> 12) & 15) as usize;
        let rs = ((instruction >> 8) & 15) as usize;
        let rm = (instruction & 15) as usize;
        if [hi, lo, rs, rm].contains(&15) || hi == lo || hi == rm || lo == rm {
            return Err(self.unsupported(instruction));
        }
        let mut result = if instruction & (1 << 22) != 0 {
            (i64::from(self.registers[rm] as i32) * i64::from(self.registers[rs] as i32)) as u64
        } else {
            u64::from(self.registers[rm]) * u64::from(self.registers[rs])
        };
        if instruction & (1 << 21) != 0 {
            let accumulator = (u64::from(self.registers[hi]) << 32) | u64::from(self.registers[lo]);
            result = result.wrapping_add(accumulator);
        }
        self.registers[hi] = (result >> 32) as u32;
        self.registers[lo] = result as u32;
        if instruction & (1 << 20) != 0 {
            self.flags.negative = result >> 63 != 0;
            self.flags.zero = result == 0;
            // Unspecified multiply C/V outputs are preserved, not hardware-verified.
        }
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }
}

fn offset_address(base: u32, offset: u32, instruction: u32) -> u32 {
    if instruction & (1 << 23) != 0 {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    }
}
