//! Original boundary programs. Expected data follows the newly fetched region,
//! not the region containing the executing instruction. Timing remains nominal.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::WAITCNT,
    memory::{ROM_CAPACITY, ROM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const UNUSED: u32 = 0x8000_0000;
const DATA: u32 = 0x9abc_1234;
const SEED: u32 = 0x5678_abcd;
const NOP: u16 = 0x46c0;
const LDR: u16 = 0x6801; // LDR r1,[r0].

fn cpu(pc: u32) -> Cpu {
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu
}

fn boundary(boundary: u32, pc: u32, instruction: u16) -> Memory {
    let mut memory = Memory::new(DATA.to_le_bytes().to_vec()).unwrap();
    if boundary < ROM_START {
        memory.write32(boundary, DATA).unwrap();
    }
    for address in [boundary - 6, boundary - 4, boundary - 2] {
        memory
            .write16(address, if address == pc { instruction } else { NOP })
            .unwrap();
    }
    memory
}

#[test]
fn mapped_video_and_rom_crossings_use_fetch_region_lanes_for_all_load_widths() {
    for boundary_address in [0x0600_0000, 0x0700_0000, 0x0800_0000] {
        for distance in [4, 2] {
            let pc = boundary_address - distance;
            let word: u32 = if boundary_address == 0x0700_0000 {
                DATA // OAM supplies a full aligned word at either Thumb alignment.
            } else if distance == 4 {
                0x1234_1234
            } else {
                0x9abc_9abc
            };
            for (instruction, width, signed) in [
                (LDR, 4, false),
                (0x7801, 1, false),
                (0x8801, 2, false),
                (0x5681, 1, true),
                (0x5e81, 2, true), // Signed loads use r2=0 offset.
            ] {
                for lane in 0..4 {
                    for warm in [false, true] {
                        let mut memory = boundary(boundary_address, pc, instruction);
                        let mut cpu = cpu(if warm { pc - 2 } else { pc });
                        cpu.registers[0] += lane;
                        cpu.flags = Flags {
                            negative: true,
                            zero: false,
                            carry: true,
                            overflow: true,
                        };
                        if warm {
                            cpu.step(&mut memory).unwrap();
                        }
                        let mut expected = cpu.clone();
                        let byte = (word >> (lane * 8)) as u8;
                        let half = (word >> ((lane & 2) * 8)) as u16;
                        expected.registers[1] = match (width, signed) {
                            (4, _) => word.rotate_right(lane * 8),
                            (1, false) => u32::from(byte),
                            (1, true) => byte as i8 as i32 as u32,
                            (2, false) => u32::from(half).rotate_right((lane & 1) * 8),
                            (2, true) if lane & 1 != 0 => byte as i8 as i32 as u32,
                            (2, true) => half as i16 as i32 as u32,
                            _ => unreachable!(),
                        };
                        expected.registers[15] += 2;
                        let timing = cpu.step_timed(&mut memory).unwrap();
                        assert_cpu_arch_eq!(
                            cpu,
                            expected,
                            "boundary={boundary_address:#x} pc={pc:#x}"
                        );
                        assert_eq!(timing.code_cycles, 1);
                        assert_eq!(timing.data_cycles, 1);
                        assert_eq!(timing.internal_cycles, 1);
                        assert_eq!(memory.cycles(), 0);
                        assert_eq!(memory.read32(UNUSED), Err(MemoryError::Unmapped(UNUSED)));
                    }
                }
            }
        }
    }
}

#[test]
fn entering_iwram_uses_actual_local_history_or_reports_unknown_lanes() {
    let boundary_address = 0x0300_0000;
    for distance in [4, 2] {
        for warm in [false, true] {
            for seed in [0, 1, 2] {
                // Unknown, CPU data read, DMA source read.
                let pc = boundary_address - distance;
                let mut memory = boundary(boundary_address, pc, LDR);
                let source = 0x0300_1000;
                memory.write32(source, SEED).unwrap();
                if seed == 1 {
                    memory.write32(0x0200_0100, 0xe592_3000).unwrap(); // ARM LDR r3,[r2].
                    let mut seeder = Cpu::new(0x0200_0100);
                    seeder.registers[2] = source;
                    seeder.step(&mut memory).unwrap();
                } else if seed == 2 {
                    let dma = DMA_BASE + 3 * DMA_STRIDE;
                    memory.write32(dma, source).unwrap();
                    memory.write32(dma + 4, 0x0200_1000).unwrap();
                    memory.write32(dma + 8, 0x8400_0001).unwrap();
                    memory.step_dma().unwrap().unwrap();
                }
                let mut cpu = cpu(if warm { pc - 2 } else { pc });
                if warm {
                    cpu.step(&mut memory).unwrap();
                }
                let before = cpu.clone();
                let result = cpu.step(&mut memory);
                if distance == 4 && seed == 0 {
                    // Only IWRAM's low half has actually been fetched.
                    assert_eq!(result, Err(CpuError::Memory(MemoryError::Unmapped(UNUSED))));
                    assert_eq!(cpu, before);
                } else {
                    result.unwrap();
                    assert_eq!(
                        cpu.registers[1],
                        if distance == 4 { 0x5678_1234 } else { DATA }
                    );
                }
            }
        }
    }
}

#[test]
fn failed_boundary_load_discards_new_samples_and_retry_fetches_current_bytes() {
    let pc = 0x02ff_fffc;
    let mut memory = boundary(0x0300_0000, pc, LDR);
    memory.write16(pc + 2, LDR).unwrap();
    let mut cpu = cpu(pc);
    let before = cpu.clone();
    assert!(cpu.step_timed(&mut memory).is_err());
    assert_eq!(cpu, before);
    memory.write16(0x0300_0000, 0x5678).unwrap();
    memory.write32(0x0200_1000, SEED).unwrap();
    cpu.registers[0] = 0x0200_1000;
    cpu.step(&mut memory).unwrap(); // Ordinary mapped load needs no complete bus snapshot.
    assert_eq!(cpu.registers[1], SEED);
    cpu.registers[0] = UNUSED;
    cpu.step(&mut memory).unwrap();
    assert_eq!(cpu.registers[1], 0x9abc_5678);
}

#[test]
fn refill_straddling_ewram_and_iwram_keeps_the_captured_target_lane() {
    let target = 0x02ff_fffe;
    let mut memory = boundary(0x0300_0000, target, LDR);
    memory.write32(0x0200_0100, 0xe12f_ff14).unwrap(); // ARM BX r4.
    let mut cpu = Cpu::new(0x0200_0100);
    cpu.registers[0] = UNUSED;
    cpu.registers[4] = target | 1;
    cpu.step(&mut memory).unwrap(); // Refill captures IWRAM's low lane (target+2).
    memory.write32(0x0300_0000, 0xdef0_5678).unwrap();
    cpu.step(&mut memory).unwrap(); // New fetch drives only the high lane.
    assert_eq!(cpu.registers[1], 0xdef0_1234);
}

#[test]
fn rom_page_and_wait_window_crossings_use_mapped_halfwords_without_changing_nominal_timing() {
    let mut rom = vec![0; ROM_CAPACITY];
    for offset in [0, 0x0100_0000] {
        rom[offset..offset + 4].copy_from_slice(&DATA.to_le_bytes());
    }
    for end in [0x0100_0000, ROM_CAPACITY] {
        for distance in [4, 2] {
            rom[end - distance..end - distance + 2].copy_from_slice(&LDR.to_le_bytes());
        }
    }
    let mut memory = Memory::new(rom).unwrap();
    for boundary in [
        0x0900_0000,
        0x0a00_0000,
        0x0b00_0000,
        0x0c00_0000,
        0x0d00_0000,
    ] {
        for distance in [4, 2] {
            for waitcnt in [0, 0x7fff] {
                memory.write16(WAITCNT, waitcnt).unwrap();
                let pc = boundary - distance;
                let mut cpu = cpu(pc);
                let timing = cpu.step_timed(&mut memory).unwrap();
                assert_eq!(
                    cpu.registers[1],
                    if distance == 4 {
                        0x1234_1234
                    } else {
                        0x9abc_9abc
                    }
                );
                assert_eq!(
                    timing.code_cycles,
                    bus_cycles(
                        memory.waitcnt(),
                        pc,
                        AccessWidth::Halfword,
                        AccessKind::Sequential
                    )
                );
                assert_eq!(timing.data_cycles, 1);
                assert_eq!(timing.internal_cycles, 1);
            }
        }
    }
}

#[test]
fn unsupported_io_and_missing_rom_lookahead_remain_diagnostic_but_branches_can_discard_them() {
    for pc in [0x03ff_fffc, 0x03ff_fffe, 0x07ff_fffc, 0x07ff_fffe] {
        let mut memory = Memory::new(vec![]).unwrap();
        memory.write16(pc, LDR).unwrap();
        let mut cpu = cpu(pc);
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
        assert_eq!(cpu, before);
        memory.write16(pc, 0x4720).unwrap(); // BX r4 discards the unsupported sequential path.
        memory.write16(0x0200_0200, 0x2107).unwrap(); // MOV r1,#7.
        cpu.registers[4] = 0x0200_0201;
        cpu.step(&mut memory).unwrap();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], 7);
    }
}

#[test]
fn a_rom_boundary_requires_only_the_fetched_halfword_not_a_full_word() {
    for length in [1, 2] {
        let mut memory = Memory::new(DATA.to_le_bytes()[..length].to_vec()).unwrap();
        let pc = ROM_START - 4;
        memory.write16(pc, LDR).unwrap();
        let mut cpu = cpu(pc);
        let before = cpu.clone();
        let result = cpu.step(&mut memory);
        if length == 1 {
            assert_eq!(result, Err(CpuError::Memory(MemoryError::Unmapped(UNUSED))));
            assert_eq!(cpu, before);
        } else {
            result.unwrap();
            assert_eq!(cpu.registers[1], 0x1234_1234);
        }
    }
}

#[test]
fn block_and_stack_loads_share_one_boundary_snapshot_and_keep_writeback() {
    for instruction in [0xcc06, 0xbc06] {
        // LDM r4!,{r1,r2}; POP {r1,r2}.
        let pc = 0x06ff_fffc;
        let mut memory = boundary(0x0700_0000, pc, instruction);
        let mut cpu = cpu(pc);
        cpu.registers[4] = UNUSED;
        cpu.registers[13] = UNUSED;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], DATA);
        assert_eq!(cpu.registers[2], DATA);
        assert_eq!(
            cpu.registers[if instruction == 0xcc06 { 4 } else { 13 }],
            UNUSED + 8
        );
        assert_eq!(cpu.pc(), pc + 2);
    }
}

#[test]
fn arm_crossings_still_use_the_new_word_and_timed_and_untimed_paths_agree() {
    for boundary in [0x0300_0000, 0x0600_0000, 0x0700_0000] {
        let pc = boundary - 8;
        let mut memory = Memory::new(vec![]).unwrap();
        memory.write32(pc, 0xe590_1000).unwrap(); // LDR r1,[r0].
        memory.write32(boundary, DATA).unwrap();
        let mut cpu = Cpu::new(pc);
        cpu.registers[0] = UNUSED;
        let mut timed = cpu.clone();
        cpu.step(&mut memory).unwrap();
        timed.step_timed(&mut memory).unwrap();
        assert_eq!(cpu, timed);
        assert_eq!(cpu.registers[1], DATA);
    }
}
