use super::{
    alu::{self, Shift},
    multiply::{self, Form},
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
        // mGBA 0.10.5 decodes MRS, MSR, and BX from bits 27-20 and 7-4 alone (`_armTable`
        // in src/arm/isa-arm.c), so their should-be-one/zero fields are ignored.
        if instruction & 0x0fb0_00f0 == 0x0100_0000 {
            return self.read_status(instruction);
        }
        if instruction & 0x0fb0_00f0 == 0x0120_0000 || instruction & 0x0fb0_0000 == 0x0320_0000 {
            return self.write_status(instruction);
        }
        if instruction & 0x0ff0_00f0 == 0x0120_0010 {
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
        // mGBA decodes SWP from bits 27-20 and 7-4, ignoring should-be-zero bits 11-8.
        if instruction & 0x0fb0_00f0 == 0x0100_0090 {
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
        // Coprocessor transfers and data operations (CDP, LDC/STC, MCR/MRC). The GBA has no
        // coprocessor; mGBA 0.10.5 runs them as stubs that change nothing (`ARM_STUB`).
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
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
        // S with Rd=15 restores SPSR where one exists; test/compare encodings then do not
        // write PC. User and System have none, so the instruction sets ordinary ALU flags,
        // as in mGBA 0.10.5.
        let restore = if destination == 15 && set_flags && self.spsr().is_some() {
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
                if instruction & (1 << 7) != 0 {
                    return Err(self.unsupported(instruction));
                }
                // A PC shift amount reads PC+8, as in mGBA 0.10.5.
                self.operand_register(shift_register, 8) & 255
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
        // Post-indexed W=1 is LDRT/STRT/LDRBT/STRBT: a user-mode access. Nothing on the GBA
        // bus depends on privilege, so it is the plain post-indexed transfer, as in mGBA 0.10.5.
        // Writeback to a PC base branches to the written address, a byte load into PC
        // branches to the byte, and a PC offset register reads PC+8, also as in mGBA.
        let offset = if instruction & (1 << 25) != 0 {
            let rm = (instruction & 15) as usize;
            if instruction & (1 << 4) != 0 {
                return Err(self.unsupported(instruction));
            }
            alu::shift(
                self.operand_register(rm, 8),
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
                memory.load32(address)?
            }
        } else {
            let value = self.operand_register(destination, 12);
            if byte {
                memory.write8(address, value as u8)?;
            } else {
                memory.store32(address, value)?;
            }
            value
        };
        if write_back && base_register != 15 {
            self.registers[base_register] = adjusted;
        }
        // ARM7 load aliases keep the loaded value, not the updated base. All
        // operands and the effective address were captured before either write.
        if load && destination != 15 {
            self.registers[destination] = value;
        }
        // ARMv4 LDR PC does not switch to Thumb, even if bit zero is set.
        self.registers[15] = if load && destination == 15 {
            value & !3
        } else if write_back && base_register == 15 {
            adjusted & !3
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
        // Post-indexed transfers always write back; W=1 there is unpredictable in ARMv4,
        // and mGBA 0.10.5 decodes it as the same transfer. Stores of signed or doubleword
        // kinds are undefined (`ILL` in mGBA). As in mGBA, PC as the destination or a
        // written-back base branches, a stored PC reads PC+12, a PC offset register reads
        // PC+8, and the register form's should-be-zero bits 11-8 are ignored.
        if kind == 0 || (!load && kind != 1) {
            return Err(self.unsupported(instruction));
        }
        let offset = if instruction & (1 << 22) != 0 {
            ((instruction >> 4) & 0xf0) | (instruction & 15)
        } else {
            self.operand_register((instruction & 15) as usize, 8)
        };
        let base = self.operand_register(base_register, 8);
        let adjusted = offset_address(base, offset, instruction);
        let address = if pre { adjusted } else { base };
        let value = if load {
            match kind {
                1 => memory.load16(address)?,
                2 => memory.read8(address)? as i8 as i32 as u32,
                _ if address & 1 != 0 => memory.read8(address)? as i8 as i32 as u32,
                _ => memory.load16(address)? as u16 as i16 as i32 as u32,
            }
        } else {
            memory.store16(address, self.operand_register(destination, 12) as u16)?;
            0
        };
        let mut next = self.pc().wrapping_add(4);
        if write_back {
            if base_register == 15 {
                next = adjusted & !3;
            } else {
                self.registers[base_register] = adjusted;
            }
        }
        // Halfword and signed load aliases also let the loaded value win.
        if load {
            if destination == 15 {
                next = value & !3;
            } else {
                self.registers[destination] = value;
            }
        }
        self.registers[15] = next;
        Ok(())
    }

    fn multiply(&mut self, instruction: u32) -> Result<(), CpuError> {
        let rd = ((instruction >> 16) & 15) as usize;
        let rn = ((instruction >> 12) & 15) as usize;
        let rs = ((instruction >> 8) & 15) as usize;
        let rm = (instruction & 15) as usize;
        let accumulate = instruction & (1 << 21) != 0;
        // MUL's Rn field should be zero; like mGBA 0.10.5, other values are ignored.
        // mGBA also skips a multiply that would write PC (Rd, or MLA's Rn) and reads a PC
        // operand as PC+8.
        if rd == 15 || (accumulate && rn == 15) {
            self.registers[15] = self.pc().wrapping_add(4);
            return Ok(());
        }
        // Rd may equal Rm: every operand is read before the destination is written.
        let (multiplicand, multiplier) =
            (self.operand_register(rm, 8), self.operand_register(rs, 8));
        let addend = if accumulate { self.registers[rn] } else { 0 };
        let result = multiplicand.wrapping_mul(multiplier).wrapping_add(addend);
        self.registers[rd] = result;
        if instruction & (1 << 20) != 0 {
            self.flags.negative = result >> 31 != 0;
            self.flags.zero = result == 0;
            self.flags.carry =
                multiply::carry(Form::Short, multiplicand, multiplier, u64::from(addend));
        }
        self.registers[15] = self.pc().wrapping_add(4);
        Ok(())
    }

    fn multiply_long(&mut self, instruction: u32) -> Result<(), CpuError> {
        let hi = ((instruction >> 16) & 15) as usize;
        let lo = ((instruction >> 12) & 15) as usize;
        let rs = ((instruction >> 8) & 15) as usize;
        let rm = (instruction & 15) as usize;
        // As in mGBA 0.10.5, a PC destination skips the multiply, a PC operand reads PC+8,
        // and with RdHi = RdLo the high word, written last, wins.
        if hi == 15 || lo == 15 {
            self.registers[15] = self.pc().wrapping_add(4);
            return Ok(());
        }
        let (multiplicand, multiplier) =
            (self.operand_register(rm, 8), self.operand_register(rs, 8));
        let signed = instruction & (1 << 22) != 0;
        let product = if signed {
            (i64::from(multiplicand as i32) * i64::from(multiplier as i32)) as u64
        } else {
            u64::from(multiplicand) * u64::from(multiplier)
        };
        let accumulator = if instruction & (1 << 21) != 0 {
            (u64::from(self.registers[hi]) << 32) | u64::from(self.registers[lo])
        } else {
            0
        };
        let result = product.wrapping_add(accumulator);
        self.registers[lo] = result as u32;
        self.registers[hi] = (result >> 32) as u32;
        if instruction & (1 << 20) != 0 {
            self.flags.negative = result >> 63 != 0;
            self.flags.zero = result == 0;
            let form = if signed {
                Form::SignedLong
            } else {
                Form::UnsignedLong
            };
            self.flags.carry = multiply::carry(form, multiplicand, multiplier, accumulator);
            // V is unaffected.
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
