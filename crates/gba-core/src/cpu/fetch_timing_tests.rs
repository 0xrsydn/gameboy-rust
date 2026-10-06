//! Original fetch-address timing tests. Source access kinds remain nominal summaries.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{IE, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    memory::{ROM_CAPACITY, ROM_START},
};

const ARM_NOP: u32 = 0xe1a0_0000;
const THUMB_NOP: u16 = 0x46c0;
const RAM: u32 = 0x0200_1000;
const DEST: u32 = 0x0300_1000;

fn program(thumb: bool, pc: u32, rom_len: usize) -> (Cpu, Memory) {
    let pattern = if thumb { 0x46c0_46c0_u32 } else { ARM_NOP };
    let mut memory = Memory::new(pattern.to_le_bytes().repeat(rom_len / 4)).unwrap();
    if pc < ROM_START {
        if thumb {
            memory.write16(pc - 2, THUMB_NOP).unwrap();
            memory.write16(pc, THUMB_NOP).unwrap();
        } else {
            memory.write32(pc - 4, ARM_NOP).unwrap();
            memory.write32(pc, ARM_NOP).unwrap();
        }
    }
    let mut cpu = Cpu::new(pc);
    if thumb {
        cpu.instruction_set = InstructionSet::Thumb;
    }
    (cpu, memory)
}

fn dma(memory: &mut Memory) {
    let base = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(base, RAM).unwrap();
    memory.write32(base + 4, DEST).unwrap();
    memory.write32(base + 8, 0x8400_0001).unwrap();
    memory.step_dma().unwrap().unwrap();
}

#[test]
fn non_refill_fetches_use_the_new_region_for_cold_and_retained_pipelines() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for (boundary, expected) in [
            (0x0300_0000, 1),                       // EWRAM -> IWRAM.
            (0x0700_0000, 1),                       // VRAM -> OAM.
            (ROM_START, if thumb { 5 } else { 8 }), // OAM -> ROM at forced-N boundary.
        ] {
            for warm in [false, true] {
                let pc = boundary - 2 * width;
                let (mut timed, mut memory) = program(thumb, pc, 32);
                let (mut plain, mut plain_memory) = program(thumb, pc, 32);
                if warm {
                    timed.registers[15] -= width;
                    plain.registers[15] -= width;
                    timed.step(&mut memory).unwrap();
                    plain.step(&mut plain_memory).unwrap();
                }
                let cost = timed.step_timed(&mut memory).unwrap();
                plain.step(&mut plain_memory).unwrap();
                assert_eq!(
                    cost.code_cycles, expected,
                    "thumb={thumb} boundary={boundary:#x}"
                );
                assert_eq!(cost.data_cycles, 0);
                assert_eq!(cost.internal_cycles, 0);
                assert_eq!(timed, plain);
                assert_eq!(memory.cycles(), 0);
            }
        }
    }
}

#[test]
fn rom_page_n_cost_occurs_at_fetch_boundary_not_two_instructions_later() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for window in 0..3 {
            for first in 0..4 {
                for second in 0..2 {
                    for prefetch in [0, 0x4000] {
                        let boundary = ROM_START + window as u32 * 0x0200_0000 + 0x20000;
                        let (mut cpu, mut memory) = program(thumb, boundary - 2 * width, 0x20020);
                        let waitcnt = (first << [2, 5, 8][window])
                            | (second << [4, 7, 10][window])
                            | prefetch;
                        memory.write16(WAITCNT, waitcnt).unwrap();
                        let n = [5, 4, 3, 9][first as usize];
                        let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                        for expected in [
                            n + if thumb { 0 } else { s },
                            if thumb { s } else { 2 * s },
                            if thumb { s } else { 2 * s },
                        ] {
                            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, expected);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn lookahead_crossing_a_rom_wait_window_uses_the_new_windows_settings() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for (boundary, window) in [(0x0a00_0000, 1), (0x0c00_0000, 2)] {
            let (cpu, mut memory) = program(thumb, boundary - 2 * width, ROM_CAPACITY);
            for first in 0..4 {
                for second in 0..2 {
                    memory
                        .write16(
                            WAITCNT,
                            (first << [2, 5, 8][window]) | (second << [4, 7, 10][window]),
                        )
                        .unwrap();
                    let mut cpu = cpu.clone();
                    let n = [5, 4, 3, 9][first as usize];
                    let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                    assert_eq!(
                        cpu.step_timed(&mut memory).unwrap().code_cycles,
                        n + if thumb { 0 } else { s }
                    );
                }
            }
        }
    }
}

#[test]
fn dma_resume_changes_the_actual_fetch_kind_without_an_extra_access() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for resumed in [false, true] {
            // Lookahead lies inside ROM, not at the forced-N page boundary.
            let (mut cpu, mut memory) = program(thumb, ROM_START - width, 32);
            if resumed {
                dma(&mut memory);
            }
            let expected = match (thumb, resumed) {
                (false, false) => 6,
                (false, true) => 8,
                (true, false) => 3,
                (true, true) => 5,
            };
            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, expected);
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap().code_cycles,
                if thumb { 3 } else { 6 }
            );
        }
    }
}

#[test]
fn a_boundary_waitcnt_store_uses_old_fetch_settings_and_the_next_fetch_uses_new_settings() {
    let mut rom = ARM_NOP.to_le_bytes().repeat(ROM_CAPACITY / 4);
    rom[ROM_CAPACITY - 4..].copy_from_slice(&0xe581_0000_u32.to_le_bytes()); // STR r0,[r1].
    let mut memory = Memory::new(rom).unwrap();
    let mut cpu = Cpu::new(0x09ff_fffc);
    cpu.registers[0] = 0xc0; // WS1 N=3, S=2.
    cpu.registers[1] = WAITCNT;
    let cost = cpu.step_timed(&mut memory).unwrap();
    assert_eq!(cost.code_cycles, 10); // Old WS1 S+S=10 at lookahead 0x0a000004.
    assert_eq!(cost.data_cycles, 1);
    assert_eq!(memory.waitcnt(), 0xc0);
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 5); // Store's next fetch: new N+S.
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 4);
}

#[test]
fn unavailable_lookahead_keeps_nominal_address_cost_and_fails_only_when_executed() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        let (mut cpu, mut memory) = program(thumb, ROM_START - width, 0);
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 3 } else { 6 }
        );
        let before = cpu.clone();
        assert_eq!(
            cpu.step_timed(&mut memory),
            Err(CpuError::Memory(MemoryError::Unmapped(ROM_START)))
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn refills_charge_one_source_fetch_and_two_target_fetches() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        let (mut cpu, mut memory) = program(thumb, ROM_START - width, 32);
        if thumb {
            memory.write16(cpu.pc(), 0x4708).unwrap();
        } else {
            memory.write32(cpu.pc(), 0xe12f_ff11).unwrap();
        }
        cpu.registers[1] = DEST | u32::from(thumb);
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 3 + 2 } else { 6 + 2 }
        );
    }
}

#[test]
fn machine_advances_boundary_fetch_cost_before_sampling_the_next_irq() {
    let (cpu, mut memory) = program(false, ROM_START - 4, 32);
    memory.write32(TIMER_BASE, 0x00c0_fffb).unwrap(); // Overflow after five cycles.
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.last_timing().code_cycles, 6);
    assert_eq!(machine.cycles(), 6);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
}
