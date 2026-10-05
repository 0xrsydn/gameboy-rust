use super::*;
use crate::memory::ROM_START;

const RAM: u32 = 0x0200_0000;
const BASE: u32 = RAM + 0x100;

fn program(words: &[u32]) -> Memory {
    Memory::new(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

fn flags() -> Flags {
    Flags {
        negative: true,
        zero: true,
        carry: true,
        overflow: true,
    }
}

fn block(mode: u32, load: bool, write_back: bool, base: u32, list: u32) -> u32 {
    0xe800_0000
        | mode
        | (u32::from(load) << 20)
        | (u32::from(write_back) << 21)
        | (base << 16)
        | list
}

// Explicit address fixtures for five-register transfers with BASE as Rn.
const MODES: [(u32, u32, u32); 4] = [
    (0x0080_0000, BASE, BASE + 20),      // IA
    (0x0180_0000, BASE + 4, BASE + 20),  // IB
    (0x0000_0000, BASE - 16, BASE - 20), // DA
    (0x0100_0000, BASE - 20, BASE - 20), // DB
];

#[test]
fn stm_all_addressing_modes_store_sparse_registers_in_ascending_order() {
    let list = (1 << 0) | (1 << 3) | (1 << 7) | (1 << 14) | (1 << 15);
    for (mode, first, updated) in MODES {
        for write_back in [false, true] {
            let mut memory = program(&[block(mode, false, write_back, 10, list)]);
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[0] = 0x11;
            cpu.registers[3] = 0x22;
            cpu.registers[7] = 0x33;
            cpu.registers[14] = 0x44;
            cpu.registers[10] = BASE;
            cpu.flags = flags();
            memory.write32(first - 4, 0xdead_beef).unwrap();
            memory.write32(first + 20, 0xcafe_babe).unwrap();
            cpu.step(&mut memory).unwrap();
            for (offset, value) in [0x11, 0x22, 0x33, 0x44, ROM_START + 12]
                .into_iter()
                .enumerate()
            {
                assert_eq!(memory.read32(first + offset as u32 * 4).unwrap(), value);
            }
            assert_eq!(memory.read32(first - 4).unwrap(), 0xdead_beef);
            assert_eq!(memory.read32(first + 20).unwrap(), 0xcafe_babe);
            assert_eq!(cpu.registers[10], if write_back { updated } else { BASE });
            assert_eq!(cpu.flags, flags());
            assert_eq!(cpu.pc(), ROM_START + 4);
        }
    }
}

#[test]
fn ldm_all_addressing_modes_load_sparse_registers_and_align_pc() {
    let registers = [0, 3, 7, 14, 15];
    let list = registers
        .iter()
        .fold(0, |list, register| list | (1 << register));
    for (mode, first, updated) in MODES {
        for write_back in [false, true] {
            let mut memory = program(&[block(mode, true, write_back, 10, list)]);
            for (offset, value) in [11, 22, 33, 44, ROM_START + 0x47].into_iter().enumerate() {
                memory.write32(first + offset as u32 * 4, value).unwrap();
            }
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[10] = BASE;
            cpu.flags = flags();
            cpu.step(&mut memory).unwrap();
            for (register, value) in registers
                .into_iter()
                .zip([11, 22, 33, 44, ROM_START + 0x44])
            {
                assert_eq!(cpu.registers[register], value);
            }
            assert_eq!(cpu.registers[10], if write_back { updated } else { BASE });
            assert_eq!(cpu.flags, flags());
        }
    }
}

#[test]
fn stm_base_in_list_stores_old_base_only_when_first_with_writeback() {
    // The five-entry list is r1,r3,r5,r7,r9. Exercise each possible base position.
    let registers = [1, 3, 5, 7, 9];
    let list = registers
        .iter()
        .fold(0, |list, register| list | (1 << register));
    for (mode, first, updated) in MODES {
        for write_back in [false, true] {
            for (position, base_register) in registers.into_iter().enumerate() {
                let mut memory = program(&[block(mode, false, write_back, base_register, list)]);
                let mut cpu = Cpu::new(ROM_START);
                cpu.registers[base_register as usize] = BASE;
                cpu.step(&mut memory).unwrap();
                let stored = memory.read32(first + position as u32 * 4).unwrap();
                assert_eq!(
                    stored,
                    if write_back && position != 0 {
                        updated
                    } else {
                        BASE
                    }
                );
                assert_eq!(
                    cpu.registers[base_register as usize],
                    if write_back { updated } else { BASE }
                );
            }
        }
    }
}

#[test]
fn ldm_base_in_list_keeps_loaded_value_instead_of_writeback() {
    let registers = [1, 3, 5, 7, 9];
    let list = registers
        .iter()
        .fold(0, |list, register| list | (1 << register));
    for (mode, first, _) in MODES {
        for write_back in [false, true] {
            for base_register in registers {
                let mut memory = program(&[block(mode, true, write_back, base_register, list)]);
                for (offset, register) in registers.into_iter().enumerate() {
                    memory
                        .write32(first + offset as u32 * 4, 100 + register)
                        .unwrap();
                }
                let mut cpu = Cpu::new(ROM_START);
                cpu.registers[base_register as usize] = BASE;
                cpu.step(&mut memory).unwrap();
                for register in registers {
                    assert_eq!(cpu.registers[register as usize], 100 + register);
                }
            }
        }
    }
}

#[test]
fn a_single_base_register_in_the_list_uses_arm7_writeback_rules() {
    for load in [false, true] {
        let mut memory = program(&[block(0x0080_0000, load, true, 2, 1 << 2)]);
        memory.write32(BASE, 0x1234_5678).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[2] = BASE;
        cpu.step(&mut memory).unwrap();
        if load {
            assert_eq!(cpu.registers[2], 0x1234_5678);
        } else {
            assert_eq!(cpu.registers[2], BASE + 4);
            assert_eq!(memory.read32(BASE).unwrap(), BASE);
        }
    }
}

#[test]
fn full_register_list_loads_all_registers_including_the_base_and_pc() {
    let mut memory = program(&[block(0x0080_0000, true, true, 10, 0xffff)]);
    for register in 0..16 {
        memory
            .write32(BASE + register * 4, 0x1000 + register * 4)
            .unwrap();
    }
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[10] = BASE;
    cpu.step(&mut memory).unwrap();
    for register in 0..16 {
        assert_eq!(cpu.registers[register], 0x1000 + register as u32 * 4);
    }
}

#[test]
fn full_register_list_stores_pc_and_updated_base_without_array_overflow() {
    let mut memory = program(&[block(0x0080_0000, false, true, 10, 0xffff)]);
    let mut cpu = Cpu::new(ROM_START);
    for register in 0..15 {
        cpu.registers[register] = register as u32;
    }
    cpu.registers[10] = BASE;
    cpu.step(&mut memory).unwrap();
    for register in 0..16 {
        let expected = match register {
            10 => BASE + 64,
            15 => ROM_START + 12,
            _ => register,
        };
        assert_eq!(memory.read32(BASE + register * 4).unwrap(), expected);
    }
}

#[test]
fn empty_list_transfers_only_pc_with_a_sixty_four_byte_span() {
    let modes = [
        (0x0080_0000, BASE, BASE + 64),
        (0x0180_0000, BASE + 4, BASE + 64),
        (0x0000_0000, BASE - 60, BASE - 64),
        (0x0100_0000, BASE - 64, BASE - 64),
    ];
    for (mode, address, updated) in modes {
        for write_back in [false, true] {
            for load in [false, true] {
                let mut memory = program(&[block(mode, load, write_back, 1, 0)]);
                memory.write32(address - 4, 0xfeed_face).unwrap();
                memory.write32(address, ROM_START + 0x47).unwrap();
                memory.write32(address + 4, 0xdead_beef).unwrap();
                let mut cpu = Cpu::new(ROM_START);
                cpu.registers[1] = BASE;
                cpu.step(&mut memory).unwrap();
                assert_eq!(cpu.registers[1], if write_back { updated } else { BASE });
                assert_eq!(cpu.pc(), ROM_START + if load { 0x44 } else { 4 });
                assert_eq!(
                    memory.read32(address).unwrap(),
                    if load {
                        ROM_START + 0x47
                    } else {
                        ROM_START + 12
                    }
                );
                assert_eq!(memory.read32(address - 4).unwrap(), 0xfeed_face);
                assert_eq!(memory.read32(address + 4).unwrap(), 0xdead_beef);
            }
        }
    }
}

#[test]
fn unaligned_block_transfers_align_words_without_rotation_and_preserve_base_low_bits() {
    for offset in 1..4 {
        let mut memory = program(&[
            block(0x0080_0000, false, true, 0, 0x6),
            block(0x0100_0000, true, true, 0, 0x18),
        ]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = BASE + offset;
        cpu.registers[1] = 0x1234_5678;
        cpu.registers[2] = 0x90ab_cdef;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], BASE + offset + 8);
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], BASE + offset);
        assert_eq!(cpu.registers[3], 0x1234_5678);
        assert_eq!(cpu.registers[4], 0x90ab_cdef);
    }
}

#[test]
fn stack_push_and_pop_restore_registers_and_return_through_pc() {
    let mut memory = program(&[
        0xe92d_4010, // STMDB sp!,{r4,lr}
        0xe3a0_4000, // MOV r4,#0
        0xe8bd_8010, // LDMIA sp!,{r4,pc}
    ]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[4] = 42;
    cpu.registers[13] = BASE;
    cpu.registers[14] = ROM_START + 20;
    for _ in 0..3 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(cpu.registers[4], 42);
    assert_eq!(cpu.registers[13], BASE);
    assert_eq!(cpu.pc(), ROM_START + 20);
    assert_eq!(memory.read32(BASE - 8).unwrap(), 42);
    assert_eq!(memory.read32(BASE - 4).unwrap(), ROM_START + 20);
}

#[test]
fn block_transfer_can_cross_ram_regions() {
    let mut memory = program(&[block(0x0080_0000, false, true, 0, 0x6)]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = 0x02ff_fffc;
    cpu.registers[1] = 11;
    cpu.registers[2] = 22;
    cpu.step(&mut memory).unwrap();
    assert_eq!(memory.read32(0x0203_fffc).unwrap(), 11); // External RAM mirror
    assert_eq!(memory.read32(0x0300_0000).unwrap(), 22); // Internal RAM
    assert_eq!(cpu.registers[0], 0x0300_0004);
}

#[test]
fn block_diagnostics_do_not_commit_partial_register_or_ram_changes() {
    for load in [false, true] {
        let mut memory = Memory::new(vec![]).unwrap();
        memory
            .write32(0x0300_0000, block(0x0080_0000, load, true, 2, 0x8001))
            .unwrap();
        memory.write32(0x07ff_fffc, 0xdead_beef).unwrap();
        let mut cpu = Cpu::new(0x0300_0000);
        cpu.registers[0] = 42;
        cpu.registers[2] = 0x07ff_fffc; // Second transfer reaches absent/read-only ROM.
        cpu.flags = flags();
        let before = cpu.registers;
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::Memory(if load {
                MemoryError::Unmapped(ROM_START)
            } else {
                MemoryError::ReadOnly(ROM_START)
            }))
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.flags, flags());
        assert_eq!(memory.read32(0x07ff_fffc).unwrap(), 0xdead_beef);
    }
}

#[test]
fn block_load_can_read_rom_and_store_reports_read_only_memory() {
    for load in [false, true] {
        let mut memory = program(&[block(0x0080_0000, load, true, 0, 0x6), 11, 22]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = ROM_START + 4;
        if load {
            cpu.step(&mut memory).unwrap();
            assert_eq!(&cpu.registers[1..3], &[11, 22]);
            assert_eq!(cpu.registers[0], ROM_START + 12);
        } else {
            let before = cpu.registers;
            assert_eq!(
                cpu.step(&mut memory),
                Err(CpuError::Memory(MemoryError::ReadOnly(ROM_START + 4)))
            );
            assert_eq!(cpu.registers, before);
            assert_eq!(memory.read32(ROM_START + 4).unwrap(), 11);
        }
    }
}

#[test]
fn truncated_rom_does_not_partially_load_a_register_list() {
    let mut memory = program(&[block(0x0080_0000, true, true, 0, 0x6), 11]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = ROM_START + 4;
    let before = cpu.registers;
    assert!(matches!(cpu.step(&mut memory), Err(CpuError::Memory(_))));
    assert_eq!(cpu.registers, before);
}

#[test]
fn swap_word_rotates_loads_and_aligns_stores_at_all_offsets() {
    for offset in 0..4 {
        let mut memory = program(&[0xe100_1092]); // SWP r1,r2,[r0]
        memory.write32(BASE, 0x1234_5678).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = BASE + offset;
        cpu.registers[2] = 0xaabb_ccdd;
        cpu.flags = flags();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], 0x1234_5678_u32.rotate_right(offset * 8));
        assert_eq!(memory.read32(BASE).unwrap(), 0xaabb_ccdd);
        assert_eq!(cpu.registers[0], BASE + offset);
        assert_eq!(cpu.registers[2], 0xaabb_ccdd);
        assert_eq!(cpu.flags, flags());
        assert_eq!(cpu.pc(), ROM_START + 4);
    }
}

#[test]
fn swap_byte_zero_extends_and_preserves_neighboring_bytes() {
    let mut memory = program(&[0xe140_1092]); // SWPB r1,r2,[r0]
    memory.write32(BASE, 0x1234_8078).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[0] = BASE + 1;
    cpu.registers[2] = 0xaabb_ccdd;
    cpu.flags = flags();
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 0x80);
    assert_eq!(memory.read32(BASE).unwrap(), 0x1234_dd78);
    assert_eq!(cpu.flags, flags());
}

#[test]
fn swap_snapshots_aliased_source_destination_and_base_registers() {
    for byte in [false, true] {
        for (destination, source) in [(1, 2), (1, 1), (1, 0), (0, 1), (0, 0)] {
            let instruction = 0xe100_0090 | (u32::from(byte) << 22) | (destination << 12) | source;
            let mut memory = program(&[instruction]);
            memory.write32(BASE, 0x1234_5678).unwrap();
            let mut cpu = Cpu::new(ROM_START);
            cpu.registers[0] = BASE;
            cpu.registers[1] = 0x1122_3344;
            cpu.registers[2] = 0x5566_7788;
            let old_source = cpu.registers[source as usize];
            cpu.step(&mut memory).unwrap();
            assert_eq!(
                cpu.registers[destination as usize],
                if byte { 0x78 } else { 0x1234_5678 }
            );
            assert_eq!(
                memory.read32(BASE).unwrap(),
                if byte {
                    0x1234_5600 | (old_source & 255)
                } else {
                    old_source
                }
            );
        }
    }
}

#[test]
fn swap_write_failure_preserves_destination_and_memory() {
    for byte in [false, true] {
        let mut memory = program(&[0xe100_1092 | (u32::from(byte) << 22), 0x1234_5678]);
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = ROM_START + 4;
        cpu.registers[1] = 123;
        cpu.registers[2] = 456;
        let before = cpu.registers;
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::Memory(MemoryError::ReadOnly(ROM_START + 4)))
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(memory.read32(ROM_START + 4).unwrap(), 0x1234_5678);
    }
}

#[test]
fn swap_read_failure_preserves_cpu_state() {
    let mut memory = program(&[0xe100_1092]);
    let mut cpu = Cpu::new(ROM_START);
    cpu.registers[1] = 42;
    let before = cpu.registers;
    assert_eq!(
        cpu.step(&mut memory),
        Err(CpuError::Memory(MemoryError::Unmapped(0)))
    );
    assert_eq!(cpu.registers, before);
}

#[test]
fn skipped_block_transfers_and_swaps_do_not_touch_unmapped_memory() {
    for instruction in [0x08a0_0006, 0x08b0_0006, 0x0100_1092, 0x0140_1092] {
        let mut memory = program(&[instruction]); // EQ while Z is clear
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[1] = 42;
        let mut expected = cpu.registers;
        expected[15] += 4;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers, expected);
    }
}

#[test]
fn unsupported_transfer_variants_preserve_registers_and_memory() {
    for instruction in [
        0xe8ef_0001, // PC base plus S bit
        0xe89f_0001, // PC base without S bit
        0xe8e0_0001, // STM user-bank transfer with writeback
        0xe8f0_0001, // LDM user-bank transfer with writeback
        0xe8fd_8000, // LDM with status restore
        0xe8c0_0000, // Empty-list STM with S bit
        0xe10f_1092, // SWP PC base
        0xe100_f092, // SWP PC destination
        0xe100_109f, // SWP PC source
        0xe110_1092, // Reserved SWP bit 20
        0xe100_1192, // Reserved SWP bits 11..8
    ] {
        let mut memory = program(&[instruction]);
        memory.write32(BASE, 0xdead_beef).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        cpu.registers[0] = BASE;
        cpu.registers[13] = BASE;
        cpu.flags = flags();
        let before = cpu.registers;
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction {
                address: ROM_START,
                instruction
            })
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.flags, flags());
        assert_eq!(memory.read32(BASE).unwrap(), 0xdead_beef);
    }
}
