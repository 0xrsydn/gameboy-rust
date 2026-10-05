//! ARMv4T Thumb instructions, including software interrupts.

use super::{
    alu::{self, Shift},
    Cpu, CpuError,
};
use crate::memory::Memory;

#[derive(Clone, Copy)]
enum Transfer {
    StoreWord,
    StoreHalf,
    StoreByte,
    LoadSignedByte,
    LoadWord,
    LoadHalf,
    LoadByte,
    LoadSignedHalf,
}

impl Cpu {
    pub(super) fn execute_thumb(
        &mut self,
        instruction: u16,
        memory: &mut Memory,
    ) -> Result<(), CpuError> {
        let instruction = u32::from(instruction);
        let next_pc = self.pc().wrapping_add(2);
        match instruction {
            0x0000..=0x17ff => {
                let source = ((instruction >> 3) & 7) as usize;
                let destination = (instruction & 7) as usize;
                let (value, carry) = alu::shift(
                    self.registers[source],
                    Shift::decode(instruction >> 11),
                    (instruction >> 6) & 31,
                    self.flags.carry,
                    false,
                );
                self.thumb_result(destination, 0xd, 0, value, carry);
            }
            0x1800..=0x1fff => {
                let source = ((instruction >> 3) & 7) as usize;
                let destination = (instruction & 7) as usize;
                let operand = (instruction >> 6) & 7;
                let operand = if instruction & (1 << 10) != 0 {
                    operand
                } else {
                    self.registers[operand as usize]
                };
                let opcode = if instruction & (1 << 9) != 0 { 2 } else { 4 };
                self.thumb_result(
                    destination,
                    opcode,
                    self.registers[source],
                    operand,
                    self.flags.carry,
                );
            }
            0x2000..=0x3fff => {
                let destination = ((instruction >> 8) & 7) as usize;
                let opcode = [0xd, 0xa, 4, 2][((instruction >> 11) & 3) as usize];
                self.thumb_result(
                    destination,
                    opcode,
                    self.registers[destination],
                    instruction & 255,
                    self.flags.carry,
                );
            }
            0x4000..=0x43ff => self.thumb_register_alu(instruction),
            0x4400..=0x47ff => return self.thumb_high_register(instruction),
            0x4800..=0x4fff => {
                let address =
                    (self.pc().wrapping_add(4) & !3).wrapping_add((instruction & 255) * 4);
                self.thumb_transfer(
                    memory,
                    Transfer::LoadWord,
                    ((instruction >> 8) & 7) as usize,
                    address,
                )?;
            }
            0x5000..=0x5fff => {
                let base = self.registers[((instruction >> 3) & 7) as usize];
                let offset = self.registers[((instruction >> 6) & 7) as usize];
                let kind = [
                    Transfer::StoreWord,
                    Transfer::StoreHalf,
                    Transfer::StoreByte,
                    Transfer::LoadSignedByte,
                    Transfer::LoadWord,
                    Transfer::LoadHalf,
                    Transfer::LoadByte,
                    Transfer::LoadSignedHalf,
                ][((instruction >> 9) & 7) as usize];
                self.thumb_transfer(
                    memory,
                    kind,
                    (instruction & 7) as usize,
                    base.wrapping_add(offset),
                )?;
            }
            0x6000..=0x7fff => {
                let byte = instruction & (1 << 12) != 0;
                let load = instruction & (1 << 11) != 0;
                let offset = ((instruction >> 6) & 31) * if byte { 1 } else { 4 };
                let base = self.registers[((instruction >> 3) & 7) as usize];
                let kind = match (load, byte) {
                    (false, false) => Transfer::StoreWord,
                    (false, true) => Transfer::StoreByte,
                    (true, false) => Transfer::LoadWord,
                    (true, true) => Transfer::LoadByte,
                };
                self.thumb_transfer(
                    memory,
                    kind,
                    (instruction & 7) as usize,
                    base.wrapping_add(offset),
                )?;
            }
            0x8000..=0x8fff => {
                let kind = if instruction & (1 << 11) != 0 {
                    Transfer::LoadHalf
                } else {
                    Transfer::StoreHalf
                };
                let base = self.registers[((instruction >> 3) & 7) as usize];
                let address = base.wrapping_add(((instruction >> 6) & 31) * 2);
                self.thumb_transfer(memory, kind, (instruction & 7) as usize, address)?;
            }
            0x9000..=0x9fff => {
                let kind = if instruction & (1 << 11) != 0 {
                    Transfer::LoadWord
                } else {
                    Transfer::StoreWord
                };
                let address = self.registers[13].wrapping_add((instruction & 255) * 4);
                self.thumb_transfer(memory, kind, ((instruction >> 8) & 7) as usize, address)?;
            }
            0xa000..=0xafff => {
                let base = if instruction & (1 << 11) != 0 {
                    self.registers[13]
                } else {
                    self.pc().wrapping_add(4) & !3
                };
                self.registers[((instruction >> 8) & 7) as usize] =
                    base.wrapping_add((instruction & 255) * 4);
            }
            0xb000..=0xb0ff => {
                let offset = (instruction & 127) * 4;
                self.registers[13] = if instruction & 128 != 0 {
                    self.registers[13].wrapping_sub(offset)
                } else {
                    self.registers[13].wrapping_add(offset)
                };
            }
            0xb400..=0xb5ff | 0xbc00..=0xbdff => {
                let pop = instruction & (1 << 11) != 0;
                let extra = if instruction & (1 << 8) != 0 {
                    1 << if pop { 15 } else { 14 }
                } else {
                    0
                };
                // Shared block logic uses Thumb PC alignment and pipeline offsets.
                let arm = if pop { 0xe8bd_0000 } else { 0xe92d_0000 };
                return self.block_transfer(arm | extra | (instruction & 255), memory);
            }
            0xc000..=0xcfff => {
                let arm = 0xe8a0_0000
                    | ((instruction & 0x800) << 9)
                    | (((instruction >> 8) & 7) << 16)
                    | (instruction & 255);
                return self.block_transfer(arm, memory);
            }
            0xdf00..=0xdfff => {
                self.enter_exception(super::Exception::SoftwareInterrupt);
                return Ok(());
            }
            0xd000..=0xddff => {
                if self.flags.condition_passed((instruction >> 8) & 15) {
                    let offset = (instruction as u8 as i8 as i32 * 2) as u32;
                    self.registers[15] = self.pc().wrapping_add(4).wrapping_add(offset);
                    return Ok(());
                }
            }
            0xe000..=0xe7ff => {
                let offset = (((instruction & 0x7ff) << 21) as i32 >> 20) as u32;
                self.registers[15] = self.pc().wrapping_add(4).wrapping_add(offset);
                return Ok(());
            }
            0xf000..=0xf7ff => {
                // BL prefix: sign-extend eleven bits, then shift left twelve.
                let offset = (((instruction & 0x7ff) << 21) as i32 >> 9) as u32;
                self.registers[14] = self.pc().wrapping_add(4).wrapping_add(offset);
            }
            0xf800..=0xffff => {
                // A standalone suffix is legal on ARM7; it uses the existing LR.
                let target = self.registers[14].wrapping_add((instruction & 0x7ff) * 2);
                self.registers[14] = next_pc | 1;
                self.registers[15] = target & !1;
                return Ok(());
            }
            // Includes undefined condition 0xE and ARMv5+ extensions.
            _ => return Err(self.unsupported(instruction)),
        }
        self.registers[15] = next_pc;
        Ok(())
    }

    fn thumb_result(&mut self, destination: usize, opcode: u32, a: u32, b: u32, carry: bool) {
        let (result, flags) = alu::execute(opcode, a, b, carry, self.flags);
        self.flags = flags;
        if !(8..=11).contains(&opcode) {
            self.registers[destination] = result;
        }
    }

    fn thumb_register_alu(&mut self, instruction: u32) {
        let destination = (instruction & 7) as usize;
        let source = ((instruction >> 3) & 7) as usize;
        let opcode = (instruction >> 6) & 15;
        let a = self.registers[destination];
        let b = self.registers[source];
        match opcode {
            2 | 3 | 4 | 7 => {
                let kind = match opcode {
                    2 => Shift::Lsl,
                    3 => Shift::Lsr,
                    4 => Shift::Asr,
                    _ => Shift::Ror,
                };
                let (value, carry) = alu::shift(a, kind, b, self.flags.carry, true);
                self.thumb_result(destination, 0xd, 0, value, carry);
            }
            9 => self.thumb_result(destination, 2, 0, b, self.flags.carry), // NEG
            13 => {
                let result = a.wrapping_mul(b);
                self.registers[destination] = result;
                self.flags.negative = result >> 31 != 0;
                self.flags.zero = result == 0;
                // Preserve unspecified multiply carry, matching the ARM core policy.
            }
            _ => self.thumb_result(destination, opcode, a, b, self.flags.carry),
        }
    }

    fn thumb_high_register(&mut self, instruction: u32) -> Result<(), CpuError> {
        let opcode = (instruction >> 8) & 3;
        let source = ((instruction >> 3) & 15) as usize;
        let destination = ((instruction & 7) | ((instruction >> 4) & 8)) as usize;
        if opcode == 3 {
            // Bit 7 selects BLX on ARMv5; low destination bits must be zero.
            if instruction & 0x87 != 0 {
                return Err(self.unsupported(instruction));
            }
            self.branch_exchange(self.operand_register(source, 4));
            return Ok(());
        }
        // ARMv4T requires at least one high register in this instruction format.
        if instruction & 0xc0 == 0 {
            return Err(self.unsupported(instruction));
        }
        let a = self.operand_register(destination, 4);
        let b = self.operand_register(source, 4);
        let value = match opcode {
            0 => a.wrapping_add(b),
            1 => {
                self.flags = alu::execute(0xa, a, b, self.flags.carry, self.flags).1;
                self.registers[15] = self.pc().wrapping_add(2);
                return Ok(());
            }
            _ => b,
        };
        if destination == 15 {
            self.registers[15] = value & !1; // ADD/MOV PC stay in Thumb state.
        } else {
            self.registers[destination] = value;
            self.registers[15] = self.pc().wrapping_add(2);
        }
        Ok(())
    }

    fn thumb_transfer(
        &mut self,
        memory: &mut Memory,
        kind: Transfer,
        register: usize,
        address: u32,
    ) -> Result<(), CpuError> {
        let value = match kind {
            Transfer::StoreWord => {
                return Ok(memory.write32(address & !3, self.registers[register])?)
            }
            Transfer::StoreHalf => {
                return Ok(memory.write16(address & !1, self.registers[register] as u16)?)
            }
            Transfer::StoreByte => {
                return Ok(memory.write8(address, self.registers[register] as u8)?)
            }
            Transfer::LoadWord => memory.read32(address & !3)?.rotate_right((address & 3) * 8),
            Transfer::LoadHalf => {
                u32::from(memory.read16(address & !1)?).rotate_right((address & 1) * 8)
            }
            Transfer::LoadByte => u32::from(memory.read8(address)?),
            Transfer::LoadSignedByte => memory.read8(address)? as i8 as i32 as u32,
            Transfer::LoadSignedHalf if address & 1 != 0 => {
                memory.read8(address)? as i8 as i32 as u32
            }
            Transfer::LoadSignedHalf => memory.read16(address)? as i16 as i32 as u32,
        };
        self.registers[register] = value;
        Ok(())
    }
}
