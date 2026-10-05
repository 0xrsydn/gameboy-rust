use super::*;
use crate::memory::ROM_START;

const RAM: u32 = 0x0200_0000;

fn program(instructions: &[u16]) -> Memory {
    Memory::new(
        instructions
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect(),
    )
    .unwrap()
}

fn thumb_cpu() -> Cpu {
    let mut cpu = Cpu::new(ROM_START);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu
}

fn flags() -> Flags {
    Flags {
        negative: true,
        zero: true,
        carry: true,
        overflow: true,
    }
}

#[test]
fn fetches_one_halfword_and_advances_two_bytes() {
    let mut memory = program(&[0x202a]); // MOV r0,#42, no trailing halfword
    let mut cpu = thumb_cpu();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[0], 42);
    assert_eq!(cpu.pc(), ROM_START + 2);
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
}

#[test]
fn immediate_shift_formats_use_arm7_zero_amount_rules() {
    for (instruction, expected, carry) in [
        (0x0008, 0x8000_0001, false), // LSL r0,r1,#0
        (0x0048, 2, true),            // LSL #1
        (0x0808, 0, true),            // LSR #32
        (0x0848, 0x4000_0000, true),  // LSR #1
        (0x1008, u32::MAX, true),     // ASR #32
        (0x1048, 0xc000_0000, true),  // ASR #1
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[1] = 0x8000_0001;
        cpu.flags.overflow = true;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
        assert_eq!(
            cpu.flags,
            Flags {
                negative: expected >> 31 != 0,
                zero: expected == 0,
                carry,
                overflow: true
            }
        );
    }
}

#[test]
fn three_register_and_small_immediate_add_subtract_formats() {
    for (instruction, expected) in [
        (0x1888, 12), // ADD r0,r1,r2
        (0x1a88, 8),  // SUB r0,r1,r2
        (0x1cc8, 13), // ADD r0,r1,#3
        (0x1ec8, 7),  // SUB r0,r1,#3
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[1] = 10;
        cpu.registers[2] = 2;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
        assert!(!cpu.flags.overflow);
        assert_eq!(cpu.flags.carry, instruction & 0x200 != 0);
    }
}

#[test]
fn add_subtract_and_compare_set_signed_overflow_and_borrow() {
    for (instruction, initial, expected, expected_flags) in [
        (
            0x3001,
            0x7fff_ffff,
            0x8000_0000,
            Flags {
                negative: true,
                zero: false,
                carry: false,
                overflow: true,
            },
        ),
        (
            0x3801,
            0x8000_0000,
            0x7fff_ffff,
            Flags {
                negative: false,
                zero: false,
                carry: true,
                overflow: true,
            },
        ),
        (
            0x3801,
            0,
            u32::MAX,
            Flags {
                negative: true,
                zero: false,
                carry: false,
                overflow: false,
            },
        ),
        (
            0x2801,
            1,
            1,
            Flags {
                negative: false,
                zero: true,
                carry: true,
                overflow: false,
            },
        ),
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[0] = initial;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
        assert_eq!(cpu.flags, expected_flags);
    }
}

#[test]
fn immediate_mov_preserves_carry_and_overflow() {
    let mut memory = program(&[0x2700]); // MOV r7,#0
    let mut cpu = thumb_cpu();
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    assert_eq!(
        cpu.flags,
        Flags {
            negative: false,
            zero: true,
            carry: true,
            overflow: true
        }
    );
}

#[test]
fn all_sixteen_register_alu_opcodes_decode_correctly() {
    let results = [
        0,
        0x36,
        0xd0,
        0xd,
        0xd,
        0x37,
        0x32,
        0xd,
        0,
        0xffff_fffe,
        0x32,
        0x36,
        0x36,
        0x68,
        0x34,
        0xffff_fffd,
    ];
    for (opcode, result) in results.into_iter().enumerate() {
        let mut memory = program(&[0x4008 | ((opcode as u16) << 6)]); // op r0,r1
        let mut cpu = thumb_cpu();
        cpu.registers[0] = 0x34;
        cpu.registers[1] = 2;
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            cpu.registers[0],
            if [8, 10, 11].contains(&opcode) {
                0x34
            } else {
                result
            },
            "opcode {opcode}"
        );
        assert_eq!(cpu.flags.negative, result >> 31 != 0);
        assert_eq!(cpu.flags.zero, result == 0);
        assert_eq!(cpu.flags.overflow, ![5, 6, 9, 10, 11].contains(&opcode));
        assert_eq!(cpu.flags.carry, ![2, 3, 4, 5, 7, 9, 11].contains(&opcode));
    }
}

#[test]
fn register_shift_amounts_include_zero_32_and_255_and_use_low_byte() {
    for (opcode, amount, expected, carry) in [
        (2, 0, 0x8000_0001, true),
        (2, 32, 0, true),
        (2, 33, 0, false),
        (3, 32, 0, true),
        (3, 255, 0, false),
        (4, 255, u32::MAX, true),
        (7, 32, 0x8000_0001, true),
        (7, 256, 0x8000_0001, true),
        (2, 257, 2, true),
    ] {
        let mut memory = program(&[0x4008 | (opcode << 6)]);
        let mut cpu = thumb_cpu();
        cpu.registers[0] = 0x8000_0001;
        cpu.registers[1] = amount;
        cpu.flags.carry = true;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
        assert_eq!(cpu.flags.carry, carry);
    }
}

#[test]
fn adc_sbc_and_neg_handle_boundary_values() {
    for (instruction, a, b, carry, expected, out_carry, overflow) in [
        (0x4148, u32::MAX, 0, true, 0, true, false),
        (0x4148, 0x7fff_ffff, 0, true, 0x8000_0000, false, true),
        (0x4188, 0, 0, false, u32::MAX, false, false),
        (0x4188, 0x8000_0000, 0, false, 0x7fff_ffff, true, true),
        (0x4248, 123, 0x8000_0000, false, 0x8000_0000, false, true),
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[0] = a;
        cpu.registers[1] = b;
        cpu.flags.carry = carry;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], expected);
        assert_eq!(cpu.flags.carry, out_carry);
        assert_eq!(cpu.flags.overflow, overflow);
    }
}

#[test]
fn multiply_wraps_and_preserves_the_unspecified_carry_policy() {
    let mut memory = program(&[0x4348]); // MUL r0,r1
    let mut cpu = thumb_cpu();
    cpu.registers[0] = 0x8000_0000;
    cpu.registers[1] = 2;
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(
        cpu.flags,
        Flags {
            negative: false,
            zero: true,
            carry: true,
            overflow: true
        }
    );
}

#[test]
fn high_register_add_and_mov_preserve_flags_while_cmp_updates_them() {
    let mut memory = program(&[
        0x4680, // MOV r8,r0
        0x4448, // ADD r0,r9
        0x4540, // CMP r0,r8
    ]);
    let mut cpu = thumb_cpu();
    cpu.registers[0] = 10;
    cpu.registers[9] = 5;
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[8], 10);
    assert_eq!(cpu.flags, flags());
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[0], 15);
    assert_eq!(cpu.flags, flags());
    cpu.step(&mut memory).unwrap();
    assert_eq!(
        cpu.flags,
        Flags {
            negative: false,
            zero: false,
            carry: true,
            overflow: false
        }
    );
}

#[test]
fn high_register_pc_reads_use_pc_plus_four_without_word_alignment() {
    for instruction in [0x4678, 0x4478] {
        // MOV/ADD r0,pc
        let mut memory = program(&[0x46c0, instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[15] += 2;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], ROM_START + 6);
    }
}

#[test]
fn high_register_pc_writes_remain_thumb_and_keep_bit_one() {
    for instruction in [0x4687, 0x4487] {
        // MOV/ADD pc,r0
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[0] = if instruction == 0x4687 {
            ROM_START + 11
        } else {
            7
        };
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), ROM_START + 10);
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
        assert_eq!(cpu.flags, flags());
    }
}

#[test]
fn thumb_bx_selects_instruction_set_from_bit_zero() {
    for (target, expected, state) in [
        (ROM_START + 11, ROM_START + 10, InstructionSet::Thumb),
        (ROM_START + 10, ROM_START + 8, InstructionSet::Arm),
    ] {
        let mut memory = program(&[0x4700]); // BX r0
        let mut cpu = thumb_cpu();
        cpu.registers[0] = target;
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), expected);
        assert_eq!(cpu.instruction_set(), state);
        assert_eq!(cpu.flags, flags());
    }
}

#[test]
fn bx_pc_switches_to_arm_with_word_alignment() {
    let mut memory = program(&[0x46c0, 0x4778]); // BX pc at halfword boundary
    let mut cpu = thumb_cpu();
    cpu.registers[15] += 2;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 4);
    assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
}

#[test]
fn literal_load_and_adr_align_pc_plus_four() {
    for instruction in [0x4801, 0xa001] {
        let mut memory = program(&[0x46c0, instruction, 0, 0, 0x5678, 0x1234]);
        let mut cpu = thumb_cpu();
        cpu.registers[15] += 2;
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            cpu.registers[0],
            if instruction == 0x4801 {
                0x1234_5678
            } else {
                ROM_START + 8
            }
        );
        assert_eq!(cpu.flags, flags());
    }
}

#[test]
fn all_register_offset_memory_formats_decode_and_handle_odd_addresses() {
    let loaded = [
        0,
        0,
        0,
        0xffff_ff80,
        0xfe12_3480,
        0xfe00_0080,
        0x80,
        0xffff_ff80,
    ];
    let stored = [0xaabb_ccdd, 0x1234_ccdd, 0x1234_ddfe];
    for opcode in 0..8 {
        let mut memory = program(&[0x5081 | (opcode << 9)]); // Rd=r1,Rb=r0,Ro=r2
        memory.write32(RAM, 0x1234_80fe).unwrap();
        let mut cpu = thumb_cpu();
        cpu.registers[0] = RAM;
        cpu.registers[1] = 0xaabb_ccdd;
        cpu.registers[2] = 1;
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        if opcode < 3 {
            assert_eq!(memory.read32(RAM).unwrap(), stored[opcode as usize]);
            assert_eq!(cpu.registers[1], 0xaabb_ccdd);
        } else {
            assert_eq!(cpu.registers[1], loaded[opcode as usize]);
            assert_eq!(memory.read32(RAM).unwrap(), 0x1234_80fe);
        }
        assert_eq!(cpu.flags, flags());
    }
}

#[test]
fn immediate_memory_offsets_scale_by_transfer_size() {
    for (store, load, address, expected) in [
        (0x6041, 0x6842, RAM + 4, 0x1234_abcd),
        (0x7041, 0x7842, RAM + 1, 0xcd),
        (0x8041, 0x8842, RAM + 2, 0xabcd),
    ] {
        let mut memory = program(&[store, load]);
        let mut cpu = thumb_cpu();
        cpu.registers[0] = RAM;
        cpu.registers[1] = 0x1234_abcd;
        cpu.step(&mut memory).unwrap();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], expected);
        assert_eq!(memory.read8(address).unwrap(), 0xcd);
    }
}

#[test]
fn sp_relative_memory_and_address_operations_preserve_flags() {
    let mut memory = program(&[
        0x9001, // STR r0,[sp,#4]
        0x9901, // LDR r1,[sp,#4]
        0xaa02, // ADD r2,sp,#8
        0xb001, // ADD sp,#4
        0xb081, // SUB sp,#4
    ]);
    let mut cpu = thumb_cpu();
    cpu.registers[13] = RAM;
    cpu.registers[0] = 42;
    cpu.flags = flags();
    for _ in 0..5 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.registers[1], 42);
    assert_eq!(cpu.registers[2], RAM + 8);
    assert_eq!(cpu.registers[13], RAM);
    assert_eq!(cpu.flags, flags());
}

#[test]
fn memory_offsets_wrap_and_sp_addition_wraps_without_panicking() {
    let mut memory = program(&[0x5081, 0xb001]); // STR r1,[r0,r2]; ADD sp,#4
    let mut cpu = thumb_cpu();
    cpu.registers[0] = 0xffff_fffc;
    cpu.registers[1] = 42;
    cpu.registers[2] = RAM + 4;
    cpu.registers[13] = u32::MAX - 3;
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(RAM).unwrap(), 42);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[13], 0);
}

#[test]
fn signed_halfword_aligned_load_sign_extends_sixteen_bits() {
    let mut memory = program(&[0x5e81]);
    memory.write16(RAM, 0x8001).unwrap();
    let mut cpu = thumb_cpu();
    cpu.registers[0] = RAM;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 0xffff_8001);
}

#[test]
fn push_and_pop_restore_registers_and_pc_without_arm_exchange() {
    for return_address in [ROM_START + 10, ROM_START + 11] {
        let mut memory = program(&[0xb505, 0x2000, 0x2200, 0xbd05]); // PUSH/POP {r0,r2,lr/pc}
        let mut cpu = thumb_cpu();
        cpu.registers[0] = 11;
        cpu.registers[2] = 22;
        cpu.registers[13] = RAM + 0x100;
        cpu.registers[14] = return_address;
        for _ in 0..4 {
            cpu.step(&mut memory).unwrap();
        }
        assert_eq!(cpu.registers[0], 11);
        assert_eq!(cpu.registers[2], 22);
        assert_eq!(cpu.registers[13], RAM + 0x100);
        assert_eq!(cpu.pc(), ROM_START + 10);
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
        assert_eq!(memory.read32(RAM + 0xf4).unwrap(), 11);
        assert_eq!(memory.read32(RAM + 0xf8).unwrap(), 22);
        assert_eq!(memory.read32(RAM + 0xfc).unwrap(), return_address);
    }
}

#[test]
fn push_pop_without_lr_or_pc_advance_by_two() {
    let mut memory = program(&[0xb401, 0xbc02]); // PUSH {r0}; POP {r1}
    let mut cpu = thumb_cpu();
    cpu.registers[0] = 123;
    cpu.registers[13] = RAM + 16;
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 123);
    assert_eq!(cpu.registers[13], RAM + 16);
    assert_eq!(cpu.pc(), ROM_START + 4);
    assert_eq!(cpu.flags, flags());
}

#[test]
fn thumb_multiple_transfers_use_low_register_base_and_ascending_addresses() {
    let mut memory = program(&[0xc305, 0xcb30]); // STMIA r3!,{r0,r2}; LDMIA r3!,{r4,r5}
    let mut cpu = thumb_cpu();
    cpu.registers[0] = 11;
    cpu.registers[2] = 22;
    cpu.registers[3] = RAM + 1;
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[3], RAM + 9);
    cpu.registers[3] = RAM + 1;
    cpu.step(&mut memory).unwrap();
    assert_eq!(&cpu.registers[4..6], &[11, 22]);
    assert_eq!(cpu.registers[3], RAM + 9);
    assert_eq!(cpu.pc(), ROM_START + 4);
    assert_eq!(cpu.flags, flags());
}

#[test]
fn thumb_multiple_transfer_base_overlap_matches_arm7() {
    for (instruction, old_base_stored) in [(0xc103, false), (0xc106, true)] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.registers[1] = RAM;
        cpu.step(&mut memory).unwrap();
        assert_eq!(
            memory
                .read32(if old_base_stored { RAM } else { RAM + 4 })
                .unwrap(),
            if old_base_stored { RAM } else { RAM + 8 }
        );
    }
    let mut memory = program(&[0xc903]); // LDMIA r1!,{r0,r1}
    memory.write32(RAM, 11).unwrap();
    memory.write32(RAM + 4, 22).unwrap();
    let mut cpu = thumb_cpu();
    cpu.registers[1] = RAM;
    cpu.step(&mut memory).unwrap();
    assert_eq!(&cpu.registers[..2], &[11, 22]);
}

#[test]
fn empty_thumb_multiple_lists_transfer_pc_with_sixty_four_byte_writeback() {
    for instruction in [0xc000, 0xc800] {
        let mut memory = program(&[instruction]);
        memory.write32(RAM, ROM_START + 11).unwrap();
        let mut cpu = thumb_cpu();
        cpu.registers[0] = RAM;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], RAM + 64);
        if instruction == 0xc000 {
            assert_eq!(memory.read32(RAM).unwrap(), ROM_START + 6);
            assert_eq!(cpu.pc(), ROM_START + 2);
        } else {
            assert_eq!(cpu.pc(), ROM_START + 10);
        }
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    }
}

#[test]
fn empty_push_pop_follow_arm7_block_transfer_rules() {
    let mut memory = program(&[0xb400, 0xbc00]);
    let mut cpu = thumb_cpu();
    cpu.registers[13] = RAM + 0x100;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[13], RAM + 0xc0);
    assert_eq!(memory.read32(RAM + 0xc0).unwrap(), ROM_START + 6);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[13], RAM + 0x100);
    assert_eq!(cpu.pc(), ROM_START + 6);
}

#[test]
fn conditional_branches_cover_every_condition_and_flag_combination() {
    let tables = [
        0xf0f0_u16, 0x0f0f, 0xcccc, 0x3333, 0xff00, 0x00ff, 0xaaaa, 0x5555, 0x0c0c, 0xf3f3, 0xaa55,
        0x55aa, 0x0a05, 0xf5fa,
    ];
    for (condition, table) in tables.into_iter().enumerate() {
        let mut memory = program(&[0xd001 | ((condition as u16) << 8)]);
        for bits in 0..16 {
            let mut cpu = thumb_cpu();
            cpu.flags = Flags {
                negative: bits & 8 != 0,
                zero: bits & 4 != 0,
                carry: bits & 2 != 0,
                overflow: bits & 1 != 0,
            };
            let before = cpu.flags;
            cpu.step(&mut memory).unwrap();
            assert_eq!(
                cpu.pc(),
                ROM_START + if table & (1 << bits) != 0 { 6 } else { 2 }
            );
            assert_eq!(cpu.flags, before);
        }
    }
}

#[test]
fn branch_offsets_are_signed_and_relative_to_pc_plus_four() {
    for (instruction, target) in [
        (0xe3ff, ROM_START + 2050),
        (0xe400, ROM_START - 2044),
        (0xe7fe, ROM_START),
        (0xd080, ROM_START - 252),
        (0xd07f, ROM_START + 258),
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.flags.zero = true;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.pc(), target);
    }
}

#[test]
fn bl_prefix_and_suffix_execute_as_separate_halfwords() {
    let mut memory = program(&[0xf000, 0xf802, 0xe7fe, 0x46c0, 0x202a, 0x4770]);
    let mut cpu = thumb_cpu();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[14], ROM_START + 4);
    assert_eq!(cpu.pc(), ROM_START + 2);
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 8);
    assert_eq!(cpu.registers[14], ROM_START + 5);
    cpu.step(&mut memory).unwrap();
    cpu.step(&mut memory).unwrap(); // BX lr
    assert_eq!(cpu.registers[0], 42);
    assert_eq!(cpu.pc(), ROM_START + 4);
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
}

#[test]
fn bl_prefix_sign_extends_and_suffix_can_execute_alone() {
    for (instruction, expected) in [
        (0xf3ff, ROM_START + 4 + 0x3ff000),
        (0xf400, ROM_START + 4 - 0x400000),
        (0xf7ff, ROM_START + 4 - 0x1000),
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[14], expected);
    }
    let mut memory = program(&[0xf801]);
    let mut cpu = thumb_cpu();
    cpu.registers[14] = ROM_START + 9;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.pc(), ROM_START + 10);
    assert_eq!(cpu.registers[14], ROM_START + 3);
}

#[test]
fn arm_thumb_arm_round_trip_fetches_the_correct_instruction_width() {
    let mut bytes: Vec<u8> = [0xe12f_ff10_u32, 0xe3a0_2063]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    bytes.extend([0x212a_u16, 0x4718].into_iter().flat_map(u16::to_le_bytes)); // MOV r1,#42; BX r3
    let mut memory = Memory::new(bytes).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = ROM_START + 9;
    cpu.registers[3] = ROM_START + 4;
    for _ in 0..4 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
    assert_eq!(cpu.registers[1], 42);
    assert_eq!(cpu.registers[2], 99);
    assert_eq!(cpu.pc(), ROM_START + 8);
}

#[test]
fn unsupported_thumb_instructions_preserve_registers_flags_and_state() {
    for instruction in [
        0xde00, 0xdeff, 0xbe00, 0xe800, 0x4780, 0x4701, 0x4400, 0x4500, 0x4600, 0xb100, 0xb200,
        0xb600, 0xbf00,
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.flags = flags();
        let before = cpu.registers;
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedThumbInstruction {
                address: ROM_START,
                instruction
            })
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.flags, flags());
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    }
}

#[test]
fn thumb_memory_errors_leave_state_unchanged() {
    for instruction in [
        0x6801, 0x6001, 0x8801, 0x8001, 0x7801, 0x7001, 0x5e81, 0x9801, 0x9001, 0x4801,
    ] {
        let mut memory = program(&[instruction]);
        let mut cpu = thumb_cpu();
        cpu.flags = flags();
        cpu.registers[1] = 123;
        let before = cpu.registers;
        assert!(
            matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))),
            "{instruction:04x}"
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.flags, flags());
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    }
}

#[test]
fn failed_thumb_store_to_rom_does_not_modify_rom_or_cpu() {
    let mut memory = program(&[0x6001]);
    let mut cpu = thumb_cpu();
    cpu.registers[0] = ROM_START;
    let before = cpu.registers;
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::ReadOnly(ROM_START)))
    );
    assert_eq!(cpu.registers, before);
    assert_eq!(memory.read16(ROM_START).unwrap(), 0x6001);
}

#[test]
fn late_thumb_stack_and_block_errors_do_not_partially_commit() {
    for instruction in [0xb403, 0xbc03, 0xc003, 0xc803] {
        let mut memory = Memory::new(vec![]).unwrap();
        memory.write16(0x0300_0000, instruction).unwrap();
        memory.write32(0x07ff_fffc, 0x1234_5678).unwrap();
        let mut cpu = thumb_cpu();
        cpu.registers[15] = 0x0300_0000;
        cpu.registers[0] = 0x07ff_fffc;
        cpu.registers[13] = if instruction == 0xb403 {
            0x0800_0004
        } else {
            0x07ff_fffc
        };
        let before = cpu.registers;
        assert!(matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))));
        assert_eq!(cpu.registers, before);
        assert_eq!(memory.read32(0x07ff_fffc).unwrap(), 0x1234_5678);
        assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
    }
}

#[test]
fn skipped_arm_bx_does_not_switch_to_thumb() {
    let mut memory = Memory::new(0x012f_ff10_u32.to_le_bytes().to_vec()).unwrap(); // BXEQ r0
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = ROM_START + 9;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.instruction_set(), InstructionSet::Arm);
    assert_eq!(cpu.pc(), ROM_START + 4);
}

#[test]
fn failing_fetch_after_bl_prefix_preserves_the_completed_prefix() {
    let mut memory = program(&[0xf000]);
    let mut cpu = thumb_cpu();
    cpu.step(&mut memory).unwrap();
    let before = cpu.registers;
    assert!(matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))));
    assert_eq!(cpu.registers, before);
    assert_eq!(cpu.registers[14], ROM_START + 4);
    assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
}

#[test]
fn every_thumb_encoding_executes_or_returns_a_diagnostic_without_panicking() {
    let mut memory = Memory::new(vec![]).unwrap();
    for instruction in 0..=u16::MAX {
        memory.write16(RAM, instruction).unwrap();
        let mut cpu = thumb_cpu();
        for register in 0..15 {
            cpu.registers[register] = RAM + 0x100 + register as u32 * 4;
        }
        cpu.registers[15] = RAM;
        cpu.flags = flags();
        let before = cpu.clone();
        match cpu.step(&mut memory) {
            Ok(()) => assert_eq!(cpu.pc() & (cpu.instruction_set.width() - 1), 0),
            Err(_) => assert_eq!(cpu, before, "encoding {instruction:04x}"),
        }
    }
}

#[test]
fn truncated_or_misaligned_thumb_fetches_do_not_advance_pc() {
    let mut memory = Memory::new(vec![0x20]).unwrap();
    let mut cpu = thumb_cpu();
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unmapped(ROM_START + 1)))
    );
    assert_eq!(cpu.pc(), ROM_START);
    cpu.registers[15] += 1;
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unaligned(ROM_START + 1)))
    );
    assert_eq!(cpu.pc(), ROM_START + 1);
}
