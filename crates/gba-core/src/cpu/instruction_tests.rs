use super::{
    alu::{self, Shift},
    *,
};
use crate::memory::ROM_START;

const RAM: u32 = 0x0200_0000;

fn program(words: &[u32]) -> Memory {
    Memory::new(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

#[test]
fn register_shifts_match_a_bit_at_a_time_reference() {
    for value in [0, 1, 0x8000_0000, 0x8000_0001, 0x1234_5678, u32::MAX] {
        for kind in [Shift::Lsl, Shift::Lsr, Shift::Asr, Shift::Ror] {
            for incoming_carry in [false, true] {
                for amount in 0..=255 {
                    let mut expected = value;
                    let mut carry = incoming_carry;
                    for _ in 0..amount {
                        match kind {
                            Shift::Lsl => {
                                carry = expected >> 31 != 0;
                                expected <<= 1;
                            }
                            Shift::Lsr => {
                                carry = expected & 1 != 0;
                                expected >>= 1;
                            }
                            Shift::Asr => {
                                carry = expected & 1 != 0;
                                expected = ((expected as i32) >> 1) as u32;
                            }
                            Shift::Ror => {
                                carry = expected & 1 != 0;
                                expected = (expected >> 1) | (u32::from(carry) << 31);
                            }
                        }
                    }
                    assert_eq!(
                        alu::shift(value, kind, amount, incoming_carry, true),
                        (expected, carry),
                        "value={value:08x} kind={kind:?} amount={amount}"
                    );
                }
            }
        }
    }
}

#[test]
fn immediate_zero_shifts_have_arm_special_meanings() {
    for carry in [false, true] {
        let value = 0x8000_0001;
        assert_eq!(
            alu::shift(value, Shift::Lsl, 0, carry, false),
            (value, carry)
        );
        assert_eq!(alu::shift(value, Shift::Lsr, 0, carry, false), (0, true));
        assert_eq!(
            alu::shift(value, Shift::Asr, 0, carry, false),
            (u32::MAX, true)
        );
        assert_eq!(
            alu::shift(value, Shift::Ror, 0, carry, false),
            (0x4000_0000 | (u32::from(carry) << 31), true)
        );
    }
    assert_eq!(alu::shift(1, Shift::Asr, 0, true, false), (0, false));
}

#[test]
fn register_shift_amount_uses_only_the_low_byte() {
    assert_eq!(alu::shift(3, Shift::Lsl, 256, true, true), (3, true));
    assert_eq!(alu::shift(3, Shift::Lsl, 257, true, true), (6, false));
    let mut memory = program(&[0xe1b0_0311]); // MOVS r0,r1,LSL r3
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[1] = 3;
    cpu.registers[3] = 257;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[0], 6);
}

#[test]
fn all_data_processing_opcodes_decode_and_preserve_test_destinations() {
    let expected = [
        0x10,
        0x26,
        0x22,
        0xffff_ffde,
        0x46,
        0x47,
        0x22,
        0xffff_ffde,
        0x10,
        0x26,
        0x22,
        0x46,
        0x36,
        0x12,
        0x24,
        0xffff_ffed,
    ];
    for (opcode, result) in expected.into_iter().enumerate() {
        // opcode{S} r2,r0,r1. Test instructions ignore the destination.
        let instruction = 0xe010_2001 | ((opcode as u32) << 21);
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = 0x34;
        cpu.registers[1] = 0x12;
        cpu.registers[2] = 0xfeed_face;
        cpu.flags.carry = true;
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            cpu.registers[2],
            if (8..=11).contains(&opcode) {
                0xfeed_face
            } else {
                result
            }
        );
        assert_eq!(cpu.flags.negative, result >> 31 != 0);
        assert_eq!(cpu.flags.zero, result == 0);
        assert_eq!(cpu.pc(), ROM_START + 4);
    }
}

#[test]
fn arithmetic_with_carry_matches_wide_signed_and_unsigned_reference() {
    let values = [0, 1, 0x7fff_ffff, 0x8000_0000, 0x8000_0001, u32::MAX];
    for opcode in [2, 3, 4, 5, 6, 7, 10, 11] {
        let mut memory = program(&[0xe010_2001 | (opcode << 21)]);
        for a in values {
            for b in values {
                for carry in [false, true] {
                    let subtract = matches!(opcode, 2 | 3 | 6 | 7 | 10);
                    let reverse = matches!(opcode, 3 | 7);
                    let (a, b) = if reverse { (b, a) } else { (a, b) };
                    let extra = match opcode {
                        5 => i64::from(carry),
                        6 | 7 => i64::from(carry) - 1,
                        _ => 0,
                    };
                    let wide = if subtract {
                        i64::from(a) - i64::from(b) + extra
                    } else {
                        i64::from(a) + i64::from(b) + extra
                    };
                    let signed = if subtract {
                        i64::from(a as i32) - i64::from(b as i32) + extra
                    } else {
                        i64::from(a as i32) + i64::from(b as i32) + extra
                    };
                    let mut cpu = Cpu::new(ROM_START);
                    (cpu.registers[0], cpu.registers[1]) = if reverse { (b, a) } else { (a, b) };
                    cpu.flags.carry = carry;
                    cpu.step(&mut memory).unwrap();
                    if opcode < 8 {
                        assert_eq!(cpu.registers[2], wide as u32);
                    }
                    assert_eq!(
                        cpu.flags,
                        Flags {
                            negative: (wide as u32) >> 31 != 0,
                            zero: wide as u32 == 0,
                            carry: if subtract {
                                wide >= 0
                            } else {
                                wide > i64::from(u32::MAX)
                            },
                            overflow: i32::try_from(signed).is_err(),
                        },
                        "opcode={opcode:x} a={a:08x} b={b:08x} carry={carry}"
                    );
                }
            }
        }
    }
}

#[test]
fn logical_ops_use_shifter_carry_and_preserve_overflow() {
    for opcode in [0, 1, 8, 9, 12, 13, 14, 15] {
        let mut memory = program(&[0xe010_2081 | (opcode << 21)]); // r1 LSL #1
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[1] = 0x8000_0001;
        cpu.flags.overflow = true;
        cpu.step(&mut memory).unwrap();
        assert!(cpu.flags.carry);
        assert!(cpu.flags.overflow);
    }
}

#[test]
fn arithmetic_uses_old_carry_not_shifter_carry() {
    let mut memory = program(&[0xe0b0_2081]); // ADCS r2,r0,r1,LSL #1
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[1] = 0x8000_0000; // shift produces carry=1 and value=0
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[2], 0);
    assert!(!cpu.flags.carry);
    assert!(cpu.flags.zero);
}

#[test]
fn non_flag_setting_register_ops_preserve_flags() {
    for opcode in [0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15] {
        let mut memory = program(&[0xe000_2081 | (opcode << 21)]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.flags = Flags {
            negative: true,
            zero: true,
            carry: true,
            overflow: true,
        };
        let before = cpu.flags;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.flags, before);
    }
}

#[test]
fn register_specified_shifts_read_pc_plus_twelve_for_both_operands() {
    for (instruction, expected) in [
        (0xe1a0_021f, ROM_START + 12), // MOV r0,pc,LSL r2
        (0xe08f_0211, ROM_START + 12), // ADD r0,pc,r1,LSL r2
        (0xe1a0_000f, ROM_START + 8),  // MOV r0,pc (immediate zero shift)
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
    }
}

#[test]
fn data_processing_pc_destination_aligns_without_switching_state() {
    let mut memory = program(&[0xe1a0_f000]); // MOV pc,r0
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM + 3;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), RAM);
}

#[test]
fn branch_link_and_bx_return_execute_a_subroutine() {
    let mut memory = program(&[
        0xe3a0_0006, // MOV r0,#6
        0xe3a0_1007, // MOV r1,#7
        0xeb00_0001, // BL subroutine at +20
        0xeaff_fffe, // B .
        0xe1a0_0000, // NOP, skipped
        0xe002_0190, // MUL r2,r0,r1
        0xe12f_ff1e, // BX lr
    ]);
    let mut cpu = Cpu::new(ROM_START);
    for _ in 0..6 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.registers[2], 42);
    assert_eq!(cpu.registers[14], ROM_START + 12);
    assert_eq!(cpu.pc(), ROM_START + 12);
}

#[test]
fn bx_thumb_target_switches_state_and_clears_only_bit_zero() {
    let mut memory = program(&[0xe12f_ff10]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = ROM_START + 11;
    let mut expected = cpu.registers;
    expected[15] = ROM_START + 10;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.flags, Flags::default());
}

#[test]
fn skipped_branch_link_does_not_change_lr() {
    let mut memory = program(&[0x0b00_0000]); // BLEQ with Z clear
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[14] = 123;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[14], 123);
    assert_eq!(cpu.pc(), ROM_START + 4);
}

#[test]
fn word_transfers_cover_pre_post_up_down_and_writeback() {
    for pre in [false, true] {
        for up in [false, true] {
            for explicit_writeback in [false, true] {
                if !pre && explicit_writeback {
                    continue;
                } // T form is intentionally unsupported.
                let fields = (u32::from(pre) << 24)
                    | (u32::from(up) << 23)
                    | (u32::from(explicit_writeback) << 21);
                let mut memory = program(&[0xe400_1004 | fields, 0xe410_2004 | fields]);
                let mut cpu = Cpu::new(ROM_START);
                let base = RAM + 16;
                let adjusted = if up { base + 4 } else { base - 4 };
                let address = if pre { adjusted } else { base };
                cpu.registers[0] = base;
                cpu.registers[1] = 0x1234_5678;
                cpu.step(&mut memory).unwrap();
                assert_eq!(memory.read32(address).unwrap(), 0x1234_5678);
                assert_eq!(
                    cpu.registers[0],
                    if !pre || explicit_writeback {
                        adjusted
                    } else {
                        base
                    }
                );
                cpu.registers[0] = base;
                cpu.step(&mut memory).unwrap();
                assert_eq!(cpu.registers[2], 0x1234_5678);
                assert_eq!(
                    cpu.registers[0],
                    if !pre || explicit_writeback {
                        adjusted
                    } else {
                        base
                    }
                );
            }
        }
    }
}

#[test]
fn byte_transfers_zero_extend_and_preserve_neighbors() {
    let mut memory = program(&[0xe5c0_1001, 0xe5d0_2001]); // STRB r1,[r0,#1]; LDRB r2,[r0,#1]
    memory.write32(RAM, 0xffff_ffff).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.registers[1] = 0x1234_5680;
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[2], 0x80);
    assert_eq!(memory.read32(RAM).unwrap(), 0xffff_80ff);
}

#[test]
fn word_loads_rotate_and_stores_align_down() {
    for offset in 0..4 {
        let mut memory = program(&[0xe590_1000, 0xe580_2000]);
        memory.write32(RAM, 0x1234_5678).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = RAM + offset;
        cpu.registers[2] = 0xaabb_ccdd;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], 0x1234_5678_u32.rotate_right(offset * 8));
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read32(RAM).unwrap(), 0xaabb_ccdd);
    }
}

#[test]
fn register_offset_load_uses_shifter_without_changing_flags() {
    let mut memory = program(&[0xe790_1102]); // LDR r1,[r0,r2,LSL #2]
    memory.write32(RAM + 12, 42).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.registers[2] = 3;
    cpu.flags.carry = true;
    let flags = cpu.flags;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 42);
    assert_eq!(cpu.flags, flags);
}

#[test]
fn pc_relative_load_and_pc_store_use_different_pipeline_offsets() {
    let mut memory = program(&[0xe59f_1000, 0xe580_f000, 42]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 42);
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(RAM).unwrap(), ROM_START + 16);
}

#[test]
fn ldr_pc_aligns_target_without_thumb_exchange() {
    let mut memory = program(&[0xe590_f000]);
    memory.write32(RAM, ROM_START + 15).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 12);
}

#[test]
fn store_base_with_writeback_stores_original_base_value() {
    let mut memory = program(&[0xe480_0004]); // STR r0,[r0],#4
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(RAM).unwrap(), RAM);
    assert_eq!(cpu.registers[0], RAM + 4);
}

#[test]
fn halfword_and_signed_loads_handle_arm7_odd_addresses() {
    for (instruction, offset, expected) in [
        (0xe1d0_10b0, 0, 0x80fe),      // LDRH
        (0xe1d0_10b0, 1, 0xfe00_0080), // odd LDRH rotates in 32 bits
        (0xe1d0_10d0, 0, 0xffff_fffe), // LDRSB
        (0xe1d0_10f0, 0, 0xffff_80fe), // LDRSH
        (0xe1d0_10f0, 1, 0xffff_ff80), // odd LDRSH becomes LDRSB
    ] {
        let mut memory = program(&[instruction]);
        memory.write16(RAM, 0x80fe).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = RAM + offset;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], expected);
    }
}

#[test]
fn halfword_store_aligns_down_and_updates_the_unaligned_base() {
    let mut memory = program(&[0xe0c0_10b2]); // STRH r1,[r0],#2
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM + 1;
    cpu.registers[1] = 0x1234_abcd;
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read16(RAM).unwrap(), 0xabcd);
    assert_eq!(cpu.registers[0], RAM + 3);
}

#[test]
fn halfword_register_offsets_and_split_immediate_are_decoded() {
    for (instruction, offset) in [(0xe190_10b2, 6), (0xe1d0_12b4, 0x24)] {
        let mut memory = program(&[instruction]);
        memory.write16(RAM + offset, 0x1234).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = RAM;
        cpu.registers[2] = 6;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], 0x1234);
    }
}

#[test]
fn multiply_and_accumulate_wrap_and_set_only_defined_flags() {
    for (instruction, expected) in [(0xe012_0190, u32::MAX - 1), (0xe032_3190, 3)] {
        let mut memory = program(&[instruction]); // MULS/MLAS r2,r0,r1[,r3]
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = u32::MAX;
        cpu.registers[1] = 2;
        cpu.registers[3] = 5;
        cpu.flags.carry = true;
        cpu.flags.overflow = true;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], expected);
        assert_eq!(cpu.flags.negative, expected >> 31 != 0);
        assert_eq!(cpu.flags.zero, expected == 0);
        assert!(cpu.flags.carry); // Deterministic policy for unspecified output.
        assert!(cpu.flags.overflow);
    }
}

#[test]
fn long_multiply_supports_signed_unsigned_and_accumulation() {
    for (instruction, expected) in [
        (0xe093_2190, 0x0000_0001_ffff_fffe_u64), // UMULLS r2,r3,r0,r1
        (0xe0b3_2190, 0x0000_0002_0000_0003),     // UMLALS (+5)
        (0xe0d3_2190, 0xffff_ffff_ffff_fffe),     // SMULLS (-1 * 2)
        (0xe0f3_2190, 3),                         // SMLALS (-2 + 5)
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = u32::MAX;
        cpu.registers[1] = 2;
        cpu.registers[2] = 5;
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            (u64::from(cpu.registers[3]) << 32) | u64::from(cpu.registers[2]),
            expected
        );
        assert_eq!(cpu.flags.negative, expected >> 63 != 0);
        assert_eq!(cpu.flags.zero, expected == 0);
    }
}

#[test]
fn long_multiply_zero_flag_uses_both_halves() {
    let mut memory = program(&[0xe093_2190]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = 0x8000_0000;
    cpu.registers[1] = 2;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[2], 0);
    assert_eq!(cpu.registers[3], 1);
    assert!(!cpu.flags.zero);
}

#[test]
fn invalid_or_unimplemented_encodings_do_not_change_state_or_memory() {
    for instruction in [
        0xe1a0_0f11, // Rs=pc (register shift)
        0xe3b0_f001, // status restore via MOVS pc
        0xe490_0004, // LDR r0,[r0],#4 (base/destination overlap)
        0xe5bf_1000, // LDR with PC writeback
        0xe4b0_1004, // LDRT user-mode transfer
        0xe5d0_f000, // LDRB pc
        0xe790_1012, // illegal register-specified memory shift
        0xe790_100f, // memory offset register PC
        0xe1c0_10d0, // ARMv5 doubleword load, not an ARM7 signed store
        0xe000_0190, // MUL Rd=Rm
        0xe082_2190, // UMULL RdHi=RdLo
        0xe8f0_0001, // LDM user-bank transfer with writeback
        0xe100_f090, // SWP with PC destination
        0xe12f_ff30, // BLX, ARMv5 only
    ] {
        let mut memory = program(&[instruction]);
        memory.write32(RAM, 0xdead_beef).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = RAM;
        cpu.registers[1] = 123;
        let before = cpu.registers;
        let flags = cpu.flags;
        assert!(
            matches!(
                cpu.step(&mut memory),
                Err(CpuError::UnsupportedInstruction { .. })
            ),
            "{instruction:08x}"
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.flags, flags);
        assert_eq!(memory.read32(RAM).unwrap(), 0xdead_beef);
    }
}

#[test]
fn failed_memory_access_does_not_commit_writeback_or_destination() {
    for instruction in [0xe490_1004, 0xe480_1004, 0xe0d0_10b2, 0xe0c0_10b2] {
        let mut memory = program(&[instruction]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[1] = 123;
        let before = cpu.registers;
        assert!(matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))));
        assert_eq!(cpu.registers, before);
    }
}

#[test]
fn conditional_store_is_skipped_without_touching_memory() {
    let mut memory = program(&[0x0580_1000]); // STREQ with Z clear
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = RAM;
    cpu.registers[1] = 123;
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(RAM).unwrap(), 0);
}
