//! Original instruction-pair tests. Expected access kinds do not use CPU summaries.
use super::*;
use crate::{
    dma::{DMA_BASE, DMA_STRIDE},
    io::{HALTCNT, IE, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    memory::{BIOS_SIZE, ROM_CAPACITY, ROM_START},
    timing::AccessKind,
};

const PC: u32 = ROM_START + 0x100;
const DATA: u32 = 0x0300_1000;
const ARM_NOP: u32 = 0xe1a0_0000;
const THUMB_NOP: u32 = 0x46c0;

fn program(thumb: bool, pc: u32, instructions: &[u32]) -> (Cpu, Memory) {
    let width = if thumb { 2 } else { 4 };
    let pattern = if thumb { 0x46c0_46c0 } else { ARM_NOP };
    let offset = (pc & 0x01ff_ffff) as usize;
    let length = (offset + 4 * width).clamp(0x20020, ROM_CAPACITY);
    let mut rom = pattern.to_le_bytes().repeat(length.div_ceil(4));
    for (index, instruction) in instructions.iter().enumerate() {
        rom[offset + index * width..offset + (index + 1) * width]
            .copy_from_slice(&instruction.to_le_bytes()[..width]);
    }
    let mut cpu = Cpu::new(pc);
    if thumb {
        cpu.instruction_set = InstructionSet::Thumb;
    }
    cpu.registers[0] = 7;
    cpu.registers[1] = DATA;
    cpu.registers[13] = DATA + 64;
    (
        cpu,
        Memory::with_bios(rom, ARM_NOP.to_le_bytes().repeat(BIOS_SIZE / 4)).unwrap(),
    )
}

#[test]
fn arm_instruction_effects_select_the_following_fetch_not_their_own() {
    for (instruction, breaks) in [
        (ARM_NOP, false),
        (0xe1a0_0082, false), // Immediate shift.
        (0xe10f_0000, false),
        (0xe321_f01f, false), // MRS / MSR.
        (0x0591_0000, false),
        (0x0581_0000, false), // Skipped load/store.
        (0x0000_0291, false),
        (0x01a0_0312, false), // Skipped multiply/shift.
        (0xe591_0000, true),
        (0xe581_0000, true), // Word load/store.
        (0xe5d1_0000, true),
        (0xe5c1_0000, true), // Byte load/store.
        (0xe1d1_00b0, true),
        (0xe1c1_00b0, true), // Halfword load/store.
        (0xe1d1_00d0, true),
        (0xe1d1_00f0, true), // Signed byte/halfword.
        (0xe891_0005, true),
        (0xe881_0005, true), // Block load/store.
        (0xe881_0000, true), // Empty block store.
        (0xe101_0092, true),
        (0xe141_0092, true), // SWP/SWPB.
        (0xe1a0_0312, true), // Register shift, zero shift amount.
        (0xe000_0392, true),
        (0xe020_4392, true), // MUL/MLA.
        (0xe085_4392, true),
        (0xe0a5_4392, true), // UMULL/UMLAL.
        (0xe0c5_4392, true),
        (0xe0e5_4392, true), // SMULL/SMLAL.
    ] {
        check_pair(false, instruction, breaks);
    }
}

#[test]
fn thumb_instruction_effects_select_the_following_fetch_not_their_own() {
    for (instruction, breaks) in [
        (THUMB_NOP, false),
        (0x0040, false),
        (0x3001, false),
        (0x4008, false),
        (0xb001, false),
        (0xf000, false), // AND, SP adjust, BL prefix.
        (0xd000, false), // Skipped BEQ (Z clear).
        (0x4800, true),  // Literal load.
        (0x6008, true),
        (0x6808, true),
        (0x7008, true),
        (0x7808, true),
        (0x8008, true),
        (0x8808, true),
        (0x9000, true),
        (0x9800, true),
        (0x5088, true),
        (0x5288, true),
        (0x5488, true),
        (0x5688, true),
        (0x5888, true),
        (0x5a88, true),
        (0x5c88, true),
        (0x5e88, true),
        (0xb405, true),
        (0xbc05, true),
        (0xc105, true),
        (0xc905, true),
        (0xb400, true),
        (0xc100, true), // Empty stores.
        (0x4090, true),
        (0x40d0, true),
        (0x4110, true),
        (0x41d0, true), // Register shifts by r2=0.
        (0x4348, true), // MUL.
    ] {
        check_pair(true, instruction, breaks);
    }
}

fn check_pair(thumb: bool, instruction: u32, breaks: bool) {
    for window in 0..3 {
        let (cpu, mut memory) = program(thumb, PC + window as u32 * 0x0200_0000, &[instruction]);
        for first in 0..4 {
            for second in 0..2 {
                for prefetch in [0, 0x4000] {
                    memory
                        .write16(
                            WAITCNT,
                            (first << [2, 5, 8][window])
                                | (second << [4, 7, 10][window])
                                | prefetch,
                        )
                        .unwrap();
                    let n = [5, 4, 3, 9][first as usize];
                    let s = if second == 1 { 2 } else { [3, 5, 9][window] };
                    let extra = if thumb { 0 } else { s };
                    let mut cpu = cpu.clone();
                    // Cold startup keeps the existing nominal S policy.
                    let first_timing = cpu.step_timed(&mut memory).unwrap();
                    assert_eq!(
                        first_timing.code_cycles,
                        s + extra,
                        "first fetch for {instruction:08x}"
                    );
                    assert_eq!(
                        cpu.pipeline.as_ref().unwrap().next_access,
                        if breaks {
                            AccessKind::NonSequential
                        } else {
                            AccessKind::Sequential
                        }
                    );
                    // RAM/internal work advances the opcode queue, independently of
                    // the CPU's N request. The Thumb literal load instead cancels it.
                    let expected = if prefetch != 0 && !(thumb && instruction == 0x4800) {
                        (s + extra)
                            .saturating_sub(first_timing.data_cycles + first_timing.internal_cycles)
                            .max(1)
                    } else if breaks {
                        n + extra
                    } else {
                        s + extra
                    };
                    assert_eq!(
                        cpu.step_timed(&mut memory).unwrap().code_cycles,
                        expected,
                        "following fetch for {instruction:08x}"
                    );
                    // Excess queued work can also accelerate a later fetch.
                    if prefetch == 0 {
                        assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, s + extra);
                    }
                    assert_eq!(memory.cycles(), 0);
                }
            }
        }
    }
}

#[test]
fn timed_and_untimed_execution_and_clones_retain_the_same_next_fetch_kind() {
    for thumb in [false, true] {
        let instruction = if thumb { 0x6808 } else { 0xe591_0000 };
        let (mut timed, mut a) = program(thumb, PC, &[instruction]);
        let (mut plain, mut b) = program(thumb, PC, &[instruction]);
        timed.step_timed(&mut a).unwrap();
        plain.step(&mut b).unwrap();
        assert_eq!(timed, plain);
        let mut copy = plain.clone();
        for expected in [if thumb { 5 } else { 8 }, if thumb { 3 } else { 6 }] {
            assert_eq!(timed.step_timed(&mut a).unwrap().code_cycles, expected);
            assert_eq!(copy.step_timed(&mut b).unwrap().code_cycles, expected);
            assert_eq!(timed, copy);
        }
    }
}

#[test]
fn following_transfers_branches_and_skipped_conditions_consume_the_prior_sequence_break() {
    for thumb in [false, true] {
        let load = if thumb { 0x6808 } else { 0xe591_0000 };
        let store = if thumb { 0x6008 } else { 0xe581_0000 };
        let branch = if thumb { 0xe000 } else { 0xea00_0000 };
        let skipped = if thumb { 0xd000 } else { 0x0581_0000 };
        for second in [load, store, branch, skipped] {
            let (mut cpu, mut memory) = program(thumb, PC, &[load, second]);
            cpu.step(&mut memory).unwrap();
            let cost = cpu.step_timed(&mut memory).unwrap();
            let n = if thumb { 5 } else { 8 };
            let s = if thumb { 3 } else { 6 };
            assert_eq!(
                cost.code_cycles,
                if second == branch { n + n + s } else { n }
            );
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap().code_cycles,
                if second == branch || second == skipped {
                    s
                } else {
                    n
                }
            );
        }
    }
}

#[test]
fn pc_load_refills_restore_sequential_fetches_after_data_and_internal_cycles() {
    for thumb in [false, true] {
        let instruction = if thumb { 0xbd00 } else { 0xe591_f000 };
        let (mut cpu, mut memory) = program(thumb, PC, &[instruction]);
        memory
            .write32(if thumb { DATA + 64 } else { DATA }, PC + 0x20)
            .unwrap();
        let cost = cpu.step_timed(&mut memory).unwrap();
        assert_eq!(cost.code_cycles, if thumb { 11 } else { 20 });
        assert_eq!(cost.data_cycles, 1);
        assert_eq!(cost.internal_cycles, 1);
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 3 } else { 6 }
        );
    }
}

#[test]
fn dma_override_coalesces_with_cpu_history_and_does_not_replay_the_penalty() {
    for thumb in [false, true] {
        for instruction in [
            if thumb { 0x6808 } else { 0xe591_0000 },
            if thumb { THUMB_NOP } else { ARM_NOP },
        ] {
            let (mut cpu, mut memory) = program(thumb, PC, &[instruction]);
            cpu.step(&mut memory).unwrap();
            let dma = DMA_BASE + 3 * DMA_STRIDE;
            memory.write32(dma, DATA).unwrap();
            memory.write32(dma + 4, DATA + 128).unwrap();
            memory.write32(dma + 8, 0x8400_0001).unwrap();
            memory.step_dma().unwrap().unwrap();
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap().code_cycles,
                if thumb { 5 } else { 8 }
            );
            assert_eq!(
                cpu.step_timed(&mut memory).unwrap().code_cycles,
                if thumb { 3 } else { 6 }
            );
        }
    }
}

#[test]
fn irq_uses_retained_incoming_kind_before_invalidating_the_interrupted_buffer() {
    for thumb in [false, true] {
        for instruction in [
            if thumb { 0x6808 } else { 0xe591_0000 },
            if thumb { THUMB_NOP } else { ARM_NOP },
        ] {
            let (mut cpu, mut memory) = program(thumb, PC, &[instruction]);
            cpu.step(&mut memory).unwrap();
            memory.write16(IE, 8).unwrap();
            memory.write16(IME, 1).unwrap();
            memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
            memory.advance_cycles(1);
            let n = instruction != if thumb { THUMB_NOP } else { ARM_NOP };
            let expected = match (thumb, n) {
                (true, true) => 7,
                (true, false) => 5,
                (false, true) => 10,
                (false, false) => 8,
            };
            let mut machine = Machine::new(cpu, memory);
            let start = machine.cycles();
            assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
            assert_eq!(machine.last_timing().code_cycles, expected);
            assert_eq!(machine.cycles() - start, u64::from(expected));
            assert_eq!(machine.cpu().next_fetch_kind(), AccessKind::Sequential);
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
            assert_eq!(machine.last_timing().code_cycles, 1);
        }
    }
}

#[test]
fn failed_execution_retains_next_kind_and_retry_does_not_consume_it() {
    for thumb in [false, true] {
        let load = if thumb { 0x6808 } else { 0xe591_0000 };
        let (mut cpu, mut memory) = program(thumb, PC, &[load, load]);
        cpu.step(&mut memory).unwrap();
        cpu.registers[1] = 0x0e00_0000;
        let before = cpu.clone();
        for _ in 0..2 {
            assert!(cpu.step_timed(&mut memory).is_err());
            assert_eq!(cpu, before);
            assert_eq!(cpu.next_fetch_kind(), AccessKind::NonSequential);
        }
        cpu.registers[1] = DATA;
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 5 } else { 8 }
        );
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 5 } else { 8 }
        );
        assert_eq!(memory.cycles(), 0);
    }
}

#[test]
fn missing_current_slot_and_masked_irq_preserve_the_pending_cpu_sequence() {
    for thumb in [false, true] {
        let instruction: u32 = if thumb { 0x6808 } else { 0xe591_0000 };
        let width = if thumb { 2 } else { 4 };
        let mut memory = Memory::new(instruction.to_le_bytes()[..width].to_vec()).unwrap();
        let mut cpu = Cpu::new(ROM_START);
        if thumb {
            cpu.instruction_set = InstructionSet::Thumb;
        }
        cpu.registers[1] = DATA;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.next_fetch_kind(), AccessKind::NonSequential);
        let before = cpu.clone();
        assert!(cpu.step_timed(&mut memory).is_err());
        assert_eq!(cpu, before);
        memory.write16(IE, 8).unwrap();
        memory.write16(IME, 1).unwrap();
        memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
        memory.advance_cycles(1);
        cpu.irq_disabled = true;
        let before = cpu.clone();
        assert_eq!(cpu.take_irq_timed(&mut memory), None);
        assert_eq!(cpu, before);
        // A debugger PC discontinuity cannot reuse the old sequence's access kind.
        cpu.registers[15] = PC;
        assert_eq!(cpu.next_fetch_kind(), AccessKind::Sequential);
    }
}

#[test]
fn host_accesses_and_halt_idle_preserve_kind_but_debugger_invalidation_starts_cold() {
    let (mut cpu, mut memory) = program(false, PC, &[0xe591_0000]);
    cpu.step(&mut memory).unwrap();
    memory.read32(DATA).unwrap();
    memory.write32(DATA, 1).unwrap();
    memory.advance_cycles(7);
    memory.write8(HALTCNT, 0).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::HaltIdle);
    assert_eq!(machine.cpu().next_fetch_kind(), AccessKind::NonSequential);
    let mut cpu = machine.cpu().clone();
    cpu.invalidate_pipeline();
    assert_eq!(cpu.next_fetch_kind(), AccessKind::Sequential);
    assert_eq!(cpu.step_timed(machine.memory_mut()).unwrap().code_cycles, 6);
}

#[test]
fn page_and_wait_window_crossings_apply_the_retained_kind_at_the_new_fetch_address() {
    for thumb in [false, true] {
        let width = if thumb { 2 } else { 4 };
        for (boundary, waitcnt, n, s) in [
            (
                ROM_START + 0x20000,
                0,
                if thumb { 5 } else { 8 },
                if thumb { 3 } else { 6 },
            ),
            (
                0x0a00_0000,
                0x80,
                if thumb { 5 } else { 7 },
                if thumb { 2 } else { 4 },
            ),
        ] {
            let (mut cpu, mut memory) = program(
                thumb,
                boundary - 3 * width,
                &[if thumb { 0x6808 } else { 0xe591_0000 }],
            );
            memory.write16(WAITCNT, waitcnt).unwrap();
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, n);
            assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, s);
        }
    }
}
