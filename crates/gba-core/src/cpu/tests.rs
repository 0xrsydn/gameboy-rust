use super::*;
use crate::memory::ROM_START;

fn program(words: &[u32]) -> Memory {
    Memory::new(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

fn flags_from_bits(bits: u32) -> Flags {
    Flags {
        negative: bits & 8 != 0,
        zero: bits & 4 != 0,
        carry: bits & 2 != 0,
        overflow: bits & 1 != 0,
    }
}

#[test]
fn all_conditions_execute_or_skip_for_every_flag_combination() {
    // Each bit marks a passing NZCV combination, indexed as N*8+Z*4+C*2+V.
    // Conditions in order: EQ NE CS CC MI PL VS VC HI LS GE LT GT LE AL.
    let truth_tables = [
        0xf0f0_u16, 0x0f0f, 0xcccc, 0x3333, 0xff00, 0x00ff, 0xaaaa, 0x5555, 0x0c0c, 0xf3f3, 0xaa55,
        0x55aa, 0x0a05, 0xf5fa, 0xffff,
    ];
    for (condition, truth_table) in truth_tables.into_iter().enumerate() {
        for instruction in [0x03a0_002a, 0x0a00_0000] {
            // MOV r0, #42; B +8
            let mut memory = program(&[instruction | ((condition as u32) << 28)]);
            for bits in 0..16 {
                let mut cpu = Cpu::new(ROM_START);
                cpu.flags = flags_from_bits(bits);
                let before = cpu.flags();
                let passes = truth_table & (1 << bits) != 0;
                cpu.step(&mut memory).unwrap();
                let branch = instruction == 0x0a00_0000;
                assert_eq!(cpu.registers()[0], if passes && !branch { 42 } else { 0 });
                assert_eq!(cpu.pc(), ROM_START + if passes && branch { 8 } else { 4 });
                assert_eq!(cpu.flags(), before);
            }
        }
    }
}

#[test]
fn arithmetic_flags_match_wide_integer_reference() {
    // Exercise every rotated-immediate encoding against boundary operands.
    let operands = [
        0,
        1,
        127,
        255,
        0x7fff_fffe,
        0x7fff_ffff,
        0x8000_0000,
        0x8000_0001,
        u32::MAX,
    ];
    let address = 0x0200_0000;
    let mut memory = Memory::new(vec![]).unwrap();
    for opcode in [0x2, 0x4, 0xa] {
        for encoding in 0..4096_u32 {
            let immediate = (encoding & 0xff).rotate_right(((encoding >> 8) & 15) * 2);
            // CMP has no destination; ADD/SUB write r1 from r0.
            let destination = if opcode == 0xa { 0 } else { 1 };
            let instruction = 0xe210_0000 | (opcode << 21) | (destination << 12) | encoding;
            for (offset, byte) in instruction.to_le_bytes().into_iter().enumerate() {
                memory.write8(address + offset as u32, byte).unwrap();
            }
            for operand in operands {
                let mut cpu = Cpu::new(address);
                cpu.registers[0] = operand;
                cpu.registers[1] = 0x1234_5678;
                cpu.flags = flags_from_bits(15);
                let mut expected_registers = *cpu.registers();
                let (unsigned_result, signed_result, carry) = if opcode == 0x4 {
                    let wide = u64::from(operand) + u64::from(immediate);
                    (
                        wide as u32,
                        i64::from(operand as i32) + i64::from(immediate as i32),
                        wide > u64::from(u32::MAX),
                    )
                } else {
                    let wide = i64::from(operand) - i64::from(immediate);
                    (
                        wide as u32,
                        i64::from(operand as i32) - i64::from(immediate as i32),
                        operand >= immediate,
                    )
                };
                if opcode != 0xa {
                    expected_registers[1] = unsigned_result;
                }
                expected_registers[15] += 4;
                cpu.step(&mut memory).unwrap();
                assert_eq!(cpu.registers(), &expected_registers);
                assert_eq!(
                    cpu.flags(),
                    Flags {
                        negative: unsigned_result >> 31 != 0,
                        zero: unsigned_result == 0,
                        carry,
                        overflow: i32::try_from(signed_result).is_err(),
                    },
                    "opcode={opcode:x} operand={operand:08x} encoding={encoding:03x}"
                );
            }
        }
    }
}

#[test]
fn movs_uses_shifter_carry_and_preserves_overflow() {
    for (instruction, result, shifted_carry) in [
        (0xe3b0_0000, 0, None),                 // MOVS r0, #0, no rotation
        (0xe3b0_00ff, 255, None),               // MOVS r0, #255, no rotation
        (0xe3b0_0480, 0x8000_0000, Some(true)), // rotated immediate
        (0xe3b0_0401, 0x0100_0000, Some(false)),
        (0xe3b0_0400, 0, Some(false)), // rotated zero still clears carry
    ] {
        let mut memory = program(&[instruction]);
        for bits in 0..16 {
            let mut cpu = Cpu::new(ROM_START);
            cpu.flags = flags_from_bits(bits);
            let before = cpu.flags();
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.registers()[0], result);
            assert_eq!(
                cpu.flags(),
                Flags {
                    negative: result >> 31 != 0,
                    zero: result == 0,
                    carry: shifted_carry.unwrap_or(before.carry),
                    overflow: before.overflow,
                }
            );
        }
    }
}

#[test]
fn instructions_without_s_preserve_all_flags() {
    for instruction in [0xe3a0_0480, 0xe280_0001, 0xe240_0001] {
        let mut memory = program(&[instruction]);
        for bits in 0..16 {
            let mut cpu = Cpu::new(ROM_START);
            cpu.flags = flags_from_bits(bits);
            let before = cpu.flags();
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.flags(), before);
        }
    }
}

#[test]
fn failed_condition_skips_flag_updates_and_unsupported_opcodes() {
    for instruction in [0x03b0_0000, 0x0f00_0000, 0x0350_0000] {
        // EQ is false, so MOVS, SWI, and CMP must all be skipped.
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.flags = flags_from_bits(11); // N=1 Z=0 C=1 V=1
        let before = cpu.flags();
        let mut registers = *cpu.registers();
        registers[15] += 4;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers(), &registers);
        assert_eq!(cpu.flags(), before);
    }
}

#[test]
fn errors_preserve_nonzero_flags_and_registers() {
    for instruction in [0xe3b0_f001, 0xe10f_f000, 0xf3a0_0001, 0xe340_0001] {
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.flags = flags_from_bits(15);
        cpu.registers[0] = 123;
        let before = *cpu.registers();
        let flags = cpu.flags();
        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction { .. })
        ));
        assert_eq!(cpu.registers(), &before);
        assert_eq!(cpu.flags(), flags);
    }
}

#[test]
fn compare_controls_a_conditional_move_without_changing_source() {
    let mut memory = program(&[
        0xe3a0_0005, // MOV r0, #5
        0xe350_0005, // CMP r0, #5
        0x03a0_102a, // MOVEQ r1, #42
        0x13a0_1063, // MOVNE r1, #99, skipped
    ]);
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..4 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(&cpu.registers()[..2], &[5, 42]);
    assert_eq!(
        cpu.flags(),
        Flags {
            negative: false,
            zero: true,
            carry: true,
            overflow: false
        }
    );
}
