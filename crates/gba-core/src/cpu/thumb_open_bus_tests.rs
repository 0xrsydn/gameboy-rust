//! Original tests for region-dependent Thumb unused-memory snapshots.
use super::*;
use crate::{
    dma::{DmaError, DMA_BASE, DMA_STRIDE},
    io::TIMER_BASE,
    machine::{Machine, MachineError, StepKind},
    memory::{BIOS_SIZE, ROM_START},
    timing::{bus_cycles, AccessKind, AccessWidth},
};

const DATA: u32 = 0xf233_80a5;
const UNUSED: u32 = 0x8000_0000;
const RAM: u32 = 0x0200_0100;
const LDR: u16 = 0x6801; // LDR r1,[r0]
const REGIONS: [u32; 11] = [
    0x100,
    RAM,
    0x0500_0100,
    0x0600_0100,
    0x0700_0100,
    ROM_START,
    0x0900_0000,
    0x0a00_0000,
    0x0b00_0000,
    0x0c00_0000,
    0x0d00_0000,
];

fn cpu(pc: u32) -> Cpu {
    let mut cpu = Cpu::new(pc);
    cpu.instruction_set = InstructionSet::Thumb;
    cpu.registers[0] = UNUSED;
    cpu
}

fn program(pc: u32, instruction: u16) -> Memory {
    let base = pc & !3;
    let mut code: Vec<u8> = [0x46c0_46c0_u32, DATA, 0xdead_beef]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let offset = (pc & 2) as usize;
    code[offset..offset + 2].copy_from_slice(&instruction.to_le_bytes());
    if pc < BIOS_SIZE as u32 {
        let mut bios = vec![0; BIOS_SIZE];
        bios[base as usize..base as usize + code.len()].copy_from_slice(&code);
        Memory::with_bios(vec![], bios).unwrap()
    } else if pc >= ROM_START {
        let offset = (base & 0x01ff_ffff) as usize;
        let mut rom = vec![0; offset + code.len()];
        rom[offset..].copy_from_slice(&code);
        Memory::new(rom).unwrap()
    } else {
        let mut bus = Memory::new(vec![]).unwrap();
        for (index, bytes) in code.chunks_exact(2).enumerate() {
            bus.write16(
                base + index as u32 * 2,
                u16::from_le_bytes(bytes.try_into().unwrap()),
            )
            .unwrap();
        }
        bus
    }
}

// Independent expected words for the bytes written above.
fn expected(pc: u32) -> u32 {
    match pc >> 24 {
        0 | 7 => DATA,
        _ if pc & 2 == 0 => 0x80a5_80a5,
        _ => 0xf233_f233,
    }
}

#[test]
fn every_supported_region_and_alignment_supplies_its_own_bus_width() {
    for base in REGIONS {
        for offset in [0, 2] {
            let pc = base + offset;
            let mut bus = program(pc, LDR);
            let mut cpu = cpu(pc);
            let timing = cpu.step_timed(&mut bus).unwrap();
            assert_eq!(cpu.registers[1], expected(pc), "pc={pc:#x}");
            assert_eq!(cpu.pc(), pc + 2);
            assert_eq!(
                timing.code_cycles,
                bus_cycles(0, pc, AccessWidth::Halfword, AccessKind::Sequential)
            );
            assert_eq!(timing.data_cycles, 1);
            assert_eq!(timing.internal_cycles, 1);
            assert_eq!(bus.cycles(), 0);
        }
    }
}

#[test]
fn loads_select_lanes_rotate_and_sign_extend_without_changing_flags() {
    for base in [0x100, RAM, 0x0700_0100, ROM_START] {
        for offset in [0, 2] {
            let pc = base + offset;
            let word = expected(pc);
            for (instruction, width, signed) in [
                (LDR, 4, false),
                (0x5881, 4, false), // Immediate/register word loads.
                (0x7801, 1, false),
                (0x8801, 2, false),
                (0x5681, 1, true),
                (0x5e81, 2, true),
            ] {
                for lane in 0..4 {
                    let byte = (word >> (lane * 8)) as u8;
                    let half = (word >> ((lane & 2) * 8)) as u16;
                    let value = match (width, signed) {
                        (4, _) => word.rotate_right(lane * 8),
                        (1, false) => u32::from(byte),
                        (1, true) => byte as i8 as i32 as u32,
                        (2, false) => u32::from(half).rotate_right((lane & 1) * 8),
                        (2, true) if lane & 1 != 0 => byte as i8 as i32 as u32,
                        (2, true) => half as i16 as i32 as u32,
                        _ => unreachable!(),
                    };
                    let mut bus = program(pc, instruction);
                    let mut cpu = cpu(pc);
                    cpu.apply_status(0xb000_00ff, Mode::System);
                    cpu.registers[0] = UNUSED + lane;
                    let mut after = cpu.clone();
                    after.registers[1] = value;
                    after.registers[15] += 2;
                    cpu.step(&mut bus).unwrap();
                    assert_cpu_arch_eq!(
                        cpu,
                        after,
                        "pc={pc:#x} instruction={instruction:#x} lane={lane}"
                    );
                }
            }
        }
    }
}

#[test]
fn unused_range_edges_are_not_mirrors_and_host_reads_remain_strict() {
    for address in [
        0x4000,
        0x00ff_fffc,
        0x0100_0000,
        0x01ff_fffc,
        0x1000_0000,
        0x1800_0000,
        UNUSED,
        0xffff_fffc,
    ] {
        let mut bus = program(ROM_START, LDR);
        let mut cpu = cpu(ROM_START);
        cpu.registers[0] = address;
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[1], 0x80a5_80a5);
        for lane in 0..4 {
            assert_eq!(
                bus.read8(address + lane),
                Err(MemoryError::Unmapped(address + lane))
            );
        }
        assert_eq!(bus.read16(address), Err(MemoryError::Unmapped(address)));
        assert_eq!(bus.read32(address), Err(MemoryError::Unmapped(address)));
        let mut invalid = super::Cpu::new(address);
        invalid.instruction_set = InstructionSet::Thumb;
        assert_eq!(
            invalid.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(address)))
        );
    }
}

#[test]
fn mirrored_code_and_physical_memory_wrap_use_mapped_fetch_bytes() {
    for base in [
        0x0204_0100,
        0x0203_fffc,
        0x0500_0500,
        0x0500_03fc,
        0x0601_8100,
        0x0601_fffc,
        0x0700_0500,
        0x0700_03fc,
    ] {
        for offset in [0, 2] {
            let pc = base + offset;
            let mut bus = program(pc, LDR);
            let mut cpu = cpu(pc);
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[1], expected(pc), "pc={pc:#x}");
        }
    }
}

#[test]
fn block_and_stack_loads_share_snapshot_and_keep_writeback_semantics() {
    for pc in [RAM, RAM + 2, 0x0700_0100, 0x0700_0102] {
        for instruction in [0xc80e, 0xc803, 0xbc06, 0xbd00, 0xc800] {
            let mut bus = program(pc, instruction);
            let mut cpu = cpu(pc);
            cpu.registers[0] = UNUSED + 3;
            cpu.registers[13] = UNUSED + 3;
            let timing = cpu.step_timed(&mut bus).unwrap();
            let value = expected(pc);
            match instruction {
                0xc80e => {
                    assert_eq!(&cpu.registers[1..4], &[value; 3]);
                    assert_eq!(cpu.registers[0], UNUSED + 15);
                    assert_eq!(timing.data_cycles, 3);
                }
                0xc803 => {
                    assert_eq!(&cpu.registers[..2], &[value; 2]); // Base in list: no writeback.
                    assert_eq!(timing.data_cycles, 2);
                }
                0xbc06 => {
                    assert_eq!(&cpu.registers[1..3], &[value; 2]);
                    assert_eq!(cpu.registers[13], UNUSED + 11);
                    assert_eq!(timing.data_cycles, 2);
                }
                0xbd00 => {
                    assert_eq!(cpu.pc(), value & !1);
                    assert_eq!(cpu.registers[13], UNUSED + 7);
                    assert_eq!(timing.data_cycles, 1);
                }
                0xc800 => {
                    assert_eq!(cpu.pc(), value & !1);
                    assert_eq!(cpu.registers[0], UNUSED + 67);
                    assert_eq!(timing.data_cycles, 1);
                }
                _ => unreachable!(),
            }
            assert_eq!(cpu.instruction_set(), InstructionSet::Thumb);
        }
        let mut bus = program(pc, 0x6800); // LDR r0,[r0], base/destination alias.
        let mut cpu = cpu(pc);
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.registers[0], expected(pc));
    }
}

#[test]
fn sequential_execution_and_branches_resample_instead_of_reusing_old_context() {
    let mut bus = program(RAM, LDR);
    bus.write16(RAM + 2, 0x6802).unwrap(); // LDR r2,[r0]
    bus.write16(RAM + 4, 0x4718).unwrap(); // BX r3
    bus.write16(0x0700_0100, LDR).unwrap();
    bus.write32(0x0700_0104, DATA).unwrap();
    let mut cpu = cpu(RAM);
    cpu.registers[3] = 0x0700_0101;
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 0x4718_4718);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], 0xf233_f233);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.pc(), 0x0700_0100);
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], DATA);
}

#[test]
fn short_rom_needs_only_the_fetched_halfword_and_errors_only_when_used() {
    for offset in [0, 2] {
        for size in offset + 2..=offset + 6 {
            let mut rom = vec![0; size];
            rom[offset..offset + 2].copy_from_slice(&LDR.to_le_bytes());
            if size == offset + 6 {
                rom[offset + 4..].copy_from_slice(&0xabcd_u16.to_le_bytes());
            }
            let mut bus = Memory::new(rom).unwrap();
            bus.write32(RAM, DATA).unwrap();
            let mut cpu = cpu(ROM_START + offset as u32);
            if size == offset + 6 {
                cpu.step(&mut bus).unwrap();
                assert_eq!(cpu.registers[1], 0xabcd_abcd);
            } else {
                let before = cpu.clone();
                assert_eq!(
                    cpu.step_timed(&mut bus),
                    Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
                );
                assert_eq!(cpu, before);
                cpu.registers[0] = RAM;
                let timing = cpu.step_timed(&mut bus).unwrap();
                assert_eq!(cpu.registers[1], DATA);
                assert_eq!(timing.data_cycles, 6);
            }
        }
    }
    let mut bus = Memory::new(0x46c0_u16.to_le_bytes().to_vec()).unwrap();
    let mut cpu = cpu(ROM_START);
    assert_eq!(cpu.step_timed(&mut bus).unwrap().data_cycles, 0);
}

#[test]
fn cold_iwram_fills_establish_lanes_but_region_crossing_fetches_remain_diagnostics() {
    for base in [0x0300_0100, 0x0300_8100, 0x02ff_fffc, 0x05ff_fffc] {
        for offset in [0, 2] {
            let pc = base + offset;
            let mut bus = program(pc, LDR);
            let mut cpu = cpu(pc);
            if pc >> 24 == 3 {
                cpu.step(&mut bus).unwrap();
                assert_eq!(
                    cpu.registers[1],
                    if offset == 0 { 0x46c0_80a5 } else { DATA }
                );
                assert_eq!(bus.read32(UNUSED), Err(MemoryError::Unmapped(UNUSED)));
                continue;
            }
            let before = cpu.clone();
            assert_eq!(
                cpu.step(&mut bus),
                Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
            );
            assert_eq!(cpu, before);
            bus.write32(RAM, DATA).unwrap();
            cpu.registers[0] = RAM;
            cpu.step(&mut bus).unwrap(); // Missing snapshot must not break ordinary loads.
            assert_eq!(cpu.registers[1], DATA);
        }
    }
    for pc in [0x3ffc, 0x3ffe] {
        let mut bios = vec![0; BIOS_SIZE];
        bios[pc..pc + 2].copy_from_slice(&LDR.to_le_bytes());
        let mut bus = Memory::with_bios(vec![], bios).unwrap();
        let mut cpu = cpu(pc as u32);
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
    }
}

#[test]
fn unsupported_data_regions_and_unused_stores_do_not_gain_fallbacks() {
    for address in [
        0,
        0x3ffc,
        0x0400_00e0,
        ROM_START + 12,
        0x0e00_0000,
        0x0fff_fffc,
    ] {
        let mut bus = program(ROM_START, LDR);
        let mut cpu = cpu(ROM_START);
        cpu.registers[0] = address;
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(address)))
        );
        assert_eq!(cpu, before);
    }
    for instruction in [0x6001, 0x7001, 0x8001, 0xc006] {
        // STR, STRB, STRH, STMIA
        let mut bus = program(ROM_START, instruction);
        let mut cpu = cpu(ROM_START);
        cpu.registers[1] = DATA;
        let before = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            Err(CpuError::Memory(MemoryError::Unmapped(UNUSED)))
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn mode_flags_and_sp_relative_addressing_do_not_select_the_open_bus_rule() {
    for mode in [
        Mode::User,
        Mode::System,
        Mode::Fiq,
        Mode::Irq,
        Mode::Supervisor,
        Mode::Abort,
        Mode::Undefined,
    ] {
        for pc in [ROM_START, ROM_START + 2, 0x0700_0100] {
            let mut bus = program(pc, 0x9900); // LDR r1,[sp]
            let mut cpu = cpu(pc);
            cpu.apply_status(0xb000_00e0 | mode as u32, mode);
            cpu.registers[13] = UNUSED;
            let status = cpu.cpsr();
            cpu.step(&mut bus).unwrap();
            assert_eq!(cpu.registers[1], expected(pc));
            assert_eq!(cpu.cpsr(), status);
            assert_eq!(cpu.registers[13], UNUSED);
        }
    }
}

#[test]
fn retained_bios_word_is_separate_from_current_thumb_unused_memory_snapshot() {
    let mut bios = vec![0; BIOS_SIZE];
    bios[0x100..0x104].copy_from_slice(&0xe12f_ff14_u32.to_le_bytes()); // BX r4
    bios[0x108..0x10c].copy_from_slice(&DATA.to_le_bytes());
    let rom: Vec<_> = [0x6802_6801_u32, 0x1234_5678]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut bus = Memory::with_bios(rom, bios).unwrap();
    let mut cpu = Cpu::new(0x100);
    cpu.registers[4] = ROM_START | 1;
    cpu.step(&mut bus).unwrap();
    cpu.step(&mut bus).unwrap(); // LDR r1,[r0=0]: retained ARM BIOS word.
    assert_eq!(cpu.registers[1], DATA);
    cpu.registers[0] = UNUSED;
    cpu.step(&mut bus).unwrap(); // LDR r2,[r0]: current Thumb ROM halfword.
    assert_eq!(cpu.registers[2], 0x1234_1234);
    assert_eq!(bus.read32(0x108).unwrap(), DATA);
}

#[test]
fn machine_timing_and_dma_remain_separate_from_thumb_snapshot_reads() {
    let mut bus = program(ROM_START, LDR);
    bus.write32(TIMER_BASE, 0x0080_0000).unwrap();
    let mut machine = Machine::new(cpu(ROM_START), bus);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[1], 0x80a5_80a5);
    assert_eq!(machine.last_timing().code_cycles, 5); // ROM boundary: 1 + 4 wait cycles.
    assert_eq!(machine.last_timing().data_cycles, 1);
    assert_eq!(machine.last_timing().internal_cycles, 1);
    assert_eq!(machine.cycles(), 7);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 7);
    let before = machine.cpu().clone();
    let timing = machine.last_timing();
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    machine.memory_mut().write32(dma, 0x0e00_0000).unwrap();
    machine.memory_mut().write32(dma + 4, RAM).unwrap();
    machine.memory_mut().write32(dma + 8, 0x8400_0001).unwrap();
    assert_eq!(
        machine.step(),
        Err(MachineError::Dma(DmaError::Memory {
            channel: 3,
            error: MemoryError::Unmapped(0x0e00_0000),
        }))
    );
    assert_eq!(machine.cpu(), &before);
    assert_eq!(machine.cycles(), 7);
    assert_eq!(machine.last_timing(), timing);
    assert_eq!(machine.memory().read32(RAM).unwrap(), 0);
}
