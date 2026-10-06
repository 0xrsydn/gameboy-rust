//! Original checks of timing-event order, independent of aggregate cycle totals.
use super::*;
use crate::{
    cpu::Cpu,
    dma::DMA_STRIDE,
    io::{IE, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    timing::TimingEvent,
};

const BASE: u32 = ROM_START + 0x100;
const THUMB: u32 = ROM_START + 0x180;
const DATA: u32 = 0x0300_1000;
const ARM_NOP: u32 = 0xe1a0_0000;

pub(super) fn prepared(
    thumb: bool,
    instructions: &[u32],
    operands: &[(usize, u32)],
) -> (Cpu, Memory) {
    let mut operands = operands.to_vec();
    if thumb {
        operands.push((12, THUMB | 1));
    }
    let mut rom = ARM_NOP.to_le_bytes().repeat(256);
    for (index, &(register, value)) in operands.iter().enumerate() {
        let at = 0x100 + 4 * index;
        let literal = 0x200 + 4 * index;
        let ldr = 0xe59f_0000 | (register as u32) << 12 | (literal - at - 8) as u32;
        rom[at..at + 4].copy_from_slice(&ldr.to_le_bytes());
        rom[literal..literal + 4].copy_from_slice(&value.to_le_bytes());
    }
    // A NOP after operand setup establishes an incoming S kind.
    let entry = 0x100 + 4 * (operands.len() + 1);
    let (entry, width) = if thumb {
        rom[entry..entry + 4].copy_from_slice(&0xe12f_ff1c_u32.to_le_bytes());
        rom[0x180..0x1c0].copy_from_slice(&0x46c0_u16.to_le_bytes().repeat(32));
        (0x180, 2)
    } else {
        (entry, 4)
    };
    for (index, instruction) in instructions.iter().enumerate() {
        rom[entry + index * width..entry + (index + 1) * width]
            .copy_from_slice(&instruction.to_le_bytes()[..width]);
    }
    let mut memory = Memory::with_bios(rom, ARM_NOP.to_le_bytes().repeat(BIOS_SIZE / 4)).unwrap();
    let mut cpu = Cpu::new(BASE);
    for _ in 0..operands.len() + 1 + usize::from(thumb) {
        cpu.step(&mut memory).unwrap();
    }
    assert!(memory.last_cpu_timing.is_none()); // Untimed setup does not create a trace.
    (cpu, memory)
}

fn code(address: u32, width: AccessWidth, kind: AccessKind, cycles: u32) -> TimingEvent {
    TimingEvent::Code {
        address,
        width,
        kind,
        waitcnt: 0,
        cycles,
    }
}

fn data(address: u32, width: AccessWidth, kind: AccessKind, cycles: u32) -> TimingEvent {
    TimingEvent::Data {
        address,
        width,
        kind,
        waitcnt: 0,
        cycles,
    }
}

fn events(memory: &Memory) -> Vec<TimingEvent> {
    assert!(memory.cpu_timing.get().is_none());
    memory.last_cpu_timing.unwrap().events()
}

#[test]
fn pc_load_records_source_data_internal_and_target_pair_in_order() {
    let target = 0x0200_0100;
    let (mut cpu, mut memory) = prepared(false, &[0xe591_f000], &[(1, DATA)]);
    memory.write32(DATA, target).unwrap();
    let source = cpu.pc() + 8;
    let timing = cpu.step_timed(&mut memory).unwrap();
    assert_eq!(
        events(&memory),
        vec![
            code(source, AccessWidth::Word, AccessKind::Sequential, 6),
            data(DATA, AccessWidth::Word, AccessKind::NonSequential, 1),
            TimingEvent::Internal { cycles: 1 },
            code(target, AccessWidth::Word, AccessKind::NonSequential, 6),
            code(target + 4, AccessWidth::Word, AccessKind::Sequential, 6),
        ]
    );
    assert_eq!(
        timing,
        StepTiming {
            code_cycles: 18,
            data_cycles: 1,
            internal_cycles: 1,
            idle_cycles: 0
        }
    );
    assert_eq!(memory.cycles(), 0);
}

#[test]
fn largest_block_load_records_all_data_before_internal_and_refill_events() {
    let target = 0x0200_0200;
    let (mut cpu, mut memory) = prepared(false, &[0xe890_ffff], &[(0, DATA)]);
    memory.write32(DATA + 60, target).unwrap();
    let mut expected = vec![code(
        cpu.pc() + 8,
        AccessWidth::Word,
        AccessKind::Sequential,
        6,
    )];
    for index in 0..16 {
        expected.push(data(
            DATA + index * 4,
            AccessWidth::Word,
            if index == 0 {
                AccessKind::NonSequential
            } else {
                AccessKind::Sequential
            },
            1,
        ));
    }
    expected.extend([
        TimingEvent::Internal { cycles: 1 },
        code(target, AccessWidth::Word, AccessKind::NonSequential, 6),
        code(target + 4, AccessWidth::Word, AccessKind::Sequential, 6),
    ]);
    let timing = cpu.step_timed(&mut memory).unwrap();
    assert_eq!(events(&memory), expected);
    assert_eq!(timing.total(), 35);
}

#[test]
fn thumb_pop_records_word_data_then_halfword_targets_without_duplicate_fetch_costs() {
    let target = 0x0300_2000;
    let (mut cpu, mut memory) = prepared(true, &[0xbd03], &[(13, DATA)]);
    memory.write32(DATA + 8, target).unwrap();
    let timing = cpu.step_timed(&mut memory).unwrap();
    assert_eq!(
        events(&memory),
        vec![
            code(THUMB + 4, AccessWidth::Halfword, AccessKind::Sequential, 3),
            data(DATA, AccessWidth::Word, AccessKind::NonSequential, 1),
            data(DATA + 4, AccessWidth::Word, AccessKind::Sequential, 1),
            data(DATA + 8, AccessWidth::Word, AccessKind::Sequential, 1),
            TimingEvent::Internal { cycles: 1 },
            code(target, AccessWidth::Halfword, AccessKind::NonSequential, 1),
            code(target + 2, AccessWidth::Halfword, AccessKind::Sequential, 1),
        ]
    );
    assert_eq!(timing.total(), 9);
}

#[test]
fn state_change_and_taken_or_skipped_branches_have_only_the_required_code_events() {
    let target = 0x0200_0100;
    let (mut cpu, mut memory) = prepared(false, &[0xe12f_ff10], &[(0, target | 1)]);
    let source = cpu.pc() + 8;
    cpu.step_timed(&mut memory).unwrap();
    assert_eq!(
        events(&memory),
        vec![
            code(source, AccessWidth::Word, AccessKind::Sequential, 6),
            code(target, AccessWidth::Halfword, AccessKind::NonSequential, 3),
            code(target + 2, AccessWidth::Halfword, AccessKind::Sequential, 3),
        ]
    );
    for (instruction, taken) in [(0xeaff_ffff, true), (0x0aff_ffff, false)] {
        let (mut cpu, mut memory) = prepared(false, &[instruction], &[]);
        let pc = cpu.pc();
        let mut expected = vec![code(pc + 8, AccessWidth::Word, AccessKind::Sequential, 6)];
        if taken {
            expected.extend([
                code(pc + 4, AccessWidth::Word, AccessKind::NonSequential, 8),
                code(pc + 8, AccessWidth::Word, AccessKind::Sequential, 6),
            ]);
        }
        cpu.step_timed(&mut memory).unwrap();
        assert_eq!(events(&memory), expected);
    }
}

#[test]
fn swaps_and_register_shifts_place_internal_cycles_after_their_bus_work() {
    for (instruction, swap) in [(0xe101_0092, true), (0xe1a0_0312, false)] {
        let (mut cpu, mut memory) = prepared(false, &[instruction], &[(1, DATA)]);
        let mut expected = vec![code(
            cpu.pc() + 8,
            AccessWidth::Word,
            AccessKind::Sequential,
            6,
        )];
        if swap {
            expected.extend([
                data(DATA, AccessWidth::Word, AccessKind::NonSequential, 1),
                data(DATA, AccessWidth::Word, AccessKind::NonSequential, 1),
            ]);
        }
        expected.push(TimingEvent::Internal { cycles: 1 });
        cpu.step_timed(&mut memory).unwrap();
        assert_eq!(events(&memory), expected);
    }
}

#[test]
fn waitcnt_store_records_old_source_settings_and_next_step_records_new_settings() {
    for waitcnt in [0x18, 0x4018] {
        // Enabling prefetch does not arm a stream before the next ROM code fetch.
        let (mut cpu, mut memory) = prepared(
            false,
            &[0xe581_0000, ARM_NOP],
            &[(0, waitcnt), (1, WAITCNT)],
        );
        let pc = cpu.pc();
        cpu.step_timed(&mut memory).unwrap();
        assert_eq!(
            events(&memory),
            vec![
                code(pc + 8, AccessWidth::Word, AccessKind::Sequential, 6),
                data(WAITCNT, AccessWidth::Word, AccessKind::NonSequential, 1),
            ]
        );
        cpu.step_timed(&mut memory).unwrap();
        assert_eq!(
            events(&memory),
            vec![TimingEvent::Code {
                address: pc + 12,
                width: AccessWidth::Word,
                kind: AccessKind::NonSequential,
                waitcnt: waitcnt as u16,
                cycles: 5,
            }]
        );
    }
}

#[test]
fn irq_entry_records_the_incoming_source_and_vector_pair_without_executing_code() {
    let (mut cpu, mut memory) = prepared(true, &[0x6808], &[(1, DATA)]);
    cpu.step_timed(&mut memory).unwrap(); // Load makes the next source N.
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    memory.advance_cycles(1);
    let mut machine = Machine::new(cpu, memory);
    let start = machine.cycles();
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(
        events(machine.memory()),
        vec![
            code(
                THUMB + 6,
                AccessWidth::Halfword,
                AccessKind::NonSequential,
                5
            ),
            code(0x18, AccessWidth::Word, AccessKind::NonSequential, 1),
            code(0x1c, AccessWidth::Word, AccessKind::Sequential, 1),
        ]
    );
    assert_eq!(machine.cycles() - start, 7);
}

#[test]
fn failed_instructions_discard_events_and_preserve_the_last_successful_trace() {
    // The second LDR reads an unsupported data region; the first retains normal events.
    let (mut cpu, mut memory) = prepared(false, &[0xe591_1000, 0xe591_0000], &[(1, DATA)]);
    memory.write32(DATA, 0x0e00_0000).unwrap();
    cpu.step_timed(&mut memory).unwrap();
    let previous = events(&memory);
    let before = cpu.clone();
    for _ in 0..2 {
        assert!(cpu.step_timed(&mut memory).is_err());
        assert_eq!(cpu, before);
        assert_eq!(events(&memory), previous);
        assert_eq!(memory.cycles(), 0);
    }
    // A late block-load error must also discard already recorded data accesses.
    {
        let (mut cpu, mut memory) = prepared(false, &[ARM_NOP, 0xe8b1_0005], &[(1, IME)]);
        cpu.step_timed(&mut memory).unwrap();
        let previous = events(&memory);
        let before = cpu.clone();
        assert!(cpu.step_timed(&mut memory).is_err());
        assert_eq!(cpu, before);
        assert_eq!(events(&memory), previous);
    }
    // Host reads and untimed code neither join nor replace a completed trace.
    memory.read32(DATA).unwrap();
    let mut probe = Cpu::new(0x0300_0000);
    probe.step(&mut memory).unwrap();
    assert_eq!(events(&memory), previous);
}

#[test]
fn deferred_source_errors_and_dma_steps_do_not_add_hidden_cpu_events() {
    let mut memory = Memory::new(ARM_NOP.to_le_bytes().to_vec()).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    cpu.step_timed(&mut memory).unwrap(); // Missing P+8 is deferred but still costs a fetch.
    let previous = vec![code(
        ROM_START + 8,
        AccessWidth::Word,
        AccessKind::Sequential,
        6,
    )];
    assert_eq!(events(&memory), previous);
    assert!(cpu.step_timed(&mut memory).is_err());
    assert_eq!(events(&memory), previous);
    let dma = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(dma, DATA).unwrap();
    memory.write32(dma + 4, DATA + 4).unwrap();
    memory.write32(dma + 8, 0x8400_0001).unwrap();
    memory.step_dma().unwrap().unwrap();
    assert_eq!(events(&memory), previous);
    assert_eq!(memory.cycles(), 4); // IWRAM read/write plus DMA startup, no CPU timing.
}
