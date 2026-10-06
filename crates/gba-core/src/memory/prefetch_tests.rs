//! Original integration checks. Expected cycles are explicit, not computed by the queue.
use super::{timing_event_tests::prepared, *};
use crate::{
    cpu::Cpu,
    dma::DMA_STRIDE,
    io::{HALTCNT, IE, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
    timing::TimingEvent,
};

const NOP: u32 = 0xe1a0_0000;
const RAM: u32 = 0x0300_1000;

fn enable(memory: &mut Memory) {
    memory.write16(WAITCNT, 0x4000).unwrap();
}

fn dma(memory: &mut Memory, source: u32, destination: u32) {
    let base = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(base, source).unwrap();
    memory.write32(base + 4, destination).unwrap();
    memory.write32(base + 8, 0x8400_0001).unwrap();
}

#[test]
fn ram_loads_and_internal_work_reduce_the_following_fetch_despite_cpu_n_kind() {
    for (thumb, instruction, address, expected) in [
        (false, 0xe591_0000, RAM, 4), // 6 ROM cycles minus RAM 1 and load 1I.
        (false, 0xe591_0000, 0x0200_1000, 1), // EWRAM 6 plus load 1I.
        (true, 0x6808, RAM, 1),       // 3 ROM cycles minus RAM 1 and load 1I.
        (false, 0xe1a0_0312, RAM, 5), // Register shift supplies 1I.
        (true, 0x4090, RAM, 2),       // Thumb register shift supplies 1I.
    ] {
        let (mut cpu, mut memory) = prepared(thumb, &[instruction, NOP], &[(1, address)]);
        enable(&mut memory);
        cpu.step_timed(&mut memory).unwrap();
        assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, expected);
        assert!(matches!(memory.last_cpu_timing.unwrap().events()[0],
            TimingEvent::Code { kind: AccessKind::NonSequential, cycles, .. } if cycles == expected));
        assert_eq!(memory.cycles(), 0);
    }
}

#[test]
fn rom_data_cancels_the_queue_and_charges_its_last_cycle_stall() {
    // An EWRAM load leaves seven free cycles. The following hit advances one more
    // cycle before ROM data cancels the active third halfword.
    let (mut cpu, mut memory) = prepared(
        false,
        &[0xe591_0000, 0xe592_0000, NOP],
        &[(1, 0x0200_1000), (2, ROM_START)],
    );
    enable(&mut memory);
    cpu.step_timed(&mut memory).unwrap(); // Seven idle-bus cycles: two entries and one cycle of the third.
    let timing = cpu.step_timed(&mut memory).unwrap(); // Hit adds one more cycle: remaining=1.
    assert_eq!(timing.code_cycles, 1);
    assert_eq!(timing.data_cycles, 9); // Raw N+S=8, cancellation=1.
    assert_eq!(timing.internal_cycles, 1);
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 8);
}

#[test]
fn branches_can_match_the_head_or_cancel_a_partial_fetch() {
    for (instruction, expected) in [(0xea00_0001, 18), (0xea00_0010, 20)] {
        let (mut cpu, mut memory) = prepared(false, &[instruction], &[]);
        enable(&mut memory);
        assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, expected);
        assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 6);
    }
    let (mut cpu, mut memory) = prepared(false, &[0xe591_f000], &[(1, RAM)]);
    memory.write32(RAM, ROM_START + 0x300).unwrap();
    enable(&mut memory);
    let timing = cpu.step_timed(&mut memory).unwrap();
    assert_eq!(timing.code_cycles, 21); // Source 6, cancelled target 9, second target 6.
    assert_eq!(timing.data_cycles + timing.internal_cycles, 2);
}

#[test]
fn swi_return_keeps_queue_ownership_separate_from_saved_instruction_state() {
    for thumb in [false, true] {
        let swi = if thumb { 0xdf00 } else { 0xef00_0000 };
        let nop = if thumb { 0x46c0 } else { NOP };
        let (mut cpu, mut memory) = prepared(thumb, &[swi, nop], &[]);
        // Original one-instruction BIOS handler: MOVS pc,lr restores the saved state.
        memory.bios.as_mut().unwrap()[8..12].copy_from_slice(&0xe1b0_f00e_u32.to_le_bytes());
        enable(&mut memory);
        let state = cpu.instruction_set();
        let return_pc = cpu.pc() + if thumb { 2 } else { 4 };
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 5 } else { 8 }
        );
        assert_eq!(cpu.pc(), 8);
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 9 } else { 15 }
        );
        assert_eq!((cpu.pc(), cpu.instruction_set()), (return_pc, state));
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            if thumb { 3 } else { 6 }
        );
        assert_eq!(memory.cycles(), 0);
    }
}

#[test]
fn timed_and_cpu_only_execution_commit_the_same_queue_without_device_clocks() {
    for thumb in [false, true] {
        let load = if thumb { 0x6808 } else { 0xe591_0000 };
        let nop = if thumb { 0x46c0 } else { NOP };
        let (mut timed, mut a) = prepared(thumb, &[load, nop, load, nop], &[(1, 0x0200_1000)]);
        let (mut plain, mut b) = prepared(thumb, &[load, nop, load, nop], &[(1, 0x0200_1000)]);
        enable(&mut a);
        enable(&mut b);
        for _ in 0..4 {
            let timing = timed.step_timed(&mut a).unwrap();
            plain.step(&mut b).unwrap();
            assert_eq!(timed, plain);
            assert_eq!(a.gamepak_prefetch, b.gamepak_prefetch);
            assert_eq!(timing, b.last_cpu_timing.unwrap().total);
            assert_eq!((a.cycles(), b.cycles()), (0, 0));
        }
    }
}

#[test]
fn failures_discard_source_and_partial_data_progress() {
    for (bad, address) in [(0xe591_0000, 0x0e00_0000), (0xe8b1_0005, IME)] {
        let (mut cpu, mut memory) = prepared(false, &[NOP, bad], &[(1, address)]);
        enable(&mut memory);
        cpu.step_timed(&mut memory).unwrap();
        let queue = memory.gamepak_prefetch;
        let previous = memory.last_cpu_timing.unwrap().events();
        let before = cpu.clone();
        for _ in 0..2 {
            assert!(cpu.step_timed(&mut memory).is_err());
            assert_eq!(cpu, before);
            assert_eq!(memory.gamepak_prefetch, queue);
            assert_eq!(memory.last_cpu_timing.unwrap().events(), previous);
            assert!(memory.cpu_timing.get().is_none());
        }
    }
}

#[test]
fn background_work_never_reads_missing_rom_bytes_or_fails_a_branch_early() {
    let mut memory = Memory::new(0xea00_003e_u32.to_le_bytes().to_vec()).unwrap();
    let mut cpu = Cpu::new(ROM_START);
    enable(&mut memory);
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 20);
    memory.advance_halt_cycles(1000); // Background addresses are beyond the file.
    let queue = memory.gamepak_prefetch;
    assert!(cpu.step_timed(&mut memory).is_err());
    assert_eq!(memory.gamepak_prefetch, queue);
}

#[test]
fn host_access_and_clock_only_updates_do_not_advance_or_consume_the_queue() {
    let (mut cpu, mut memory) = prepared(false, &[NOP, NOP], &[]);
    enable(&mut memory);
    cpu.step_timed(&mut memory).unwrap();
    let queue = memory.gamepak_prefetch;
    memory.read32(ROM_START).unwrap();
    memory.write32(RAM, NOP).unwrap();
    memory.advance_cycles(1000);
    assert_eq!(memory.gamepak_prefetch, queue);
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 6);
}

#[test]
fn debugger_invalidation_discards_queued_timing_on_success() {
    let (mut cpu, mut memory) = prepared(false, &[0xe591_0000, NOP], &[(1, 0x0200_1000)]);
    enable(&mut memory);
    cpu.step_timed(&mut memory).unwrap();
    cpu.invalidate_pipeline();
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 6); // Cold nominal S, not a hit.
}

#[test]
fn cpu_waitcnt_write_resets_after_the_old_source_and_data_costs() {
    for new in [0, 0x4018, 0x4001, 0x4800] {
        let (mut cpu, mut memory) = prepared(false, &[0xe581_0000, NOP], &[(0, new), (1, WAITCNT)]);
        enable(&mut memory);
        let old = cpu.step_timed(&mut memory).unwrap();
        assert_eq!((old.code_cycles, old.data_cycles), (6, 1));
        assert_eq!(memory.waitcnt(), new as u16);
        let expected = match new {
            0 => 8,      // Disabled: CPU N request.
            0x4018 => 5, // Changed waits reset queue: new N+S.
            _ => 5,      // SRAM/PHI keep queue: 6 minus the store's free cycle.
        };
        assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, expected);
    }
}

#[test]
fn host_byte_and_dma_waitcnt_writes_apply_queue_configuration() {
    let (mut cpu, mut memory) = prepared(false, &[NOP, NOP, NOP], &[]);
    enable(&mut memory);
    cpu.step_timed(&mut memory).unwrap();
    memory.write8(WAITCNT, 0x18).unwrap();
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 4);
    memory.write32(RAM, 0).unwrap();
    dma(&mut memory, RAM, WAITCNT);
    memory.step_dma().unwrap().unwrap();
    assert_eq!(memory.waitcnt(), 0);
    assert_eq!(cpu.step_timed(&mut memory).unwrap().code_cycles, 8);
}

#[test]
fn ram_dma_advances_the_queue_and_rom_dma_cancels_it_before_cpu_resume() {
    for (source, destination, expected_dma, expected_cpu) in [
        (RAM, RAM + 4, 4, 2),     // Startup 2 + RAM 1+1 gives four free cycles.
        (0x0200_1000, RAM, 9, 1), // EWRAM gives a complete word.
        (ROM_START, RAM, 12, 8),  // Startup leaves remaining=1: ROM data stalls one cycle.
    ] {
        let (mut cpu, mut memory) = prepared(false, &[NOP, NOP], &[]);
        enable(&mut memory);
        cpu.step_timed(&mut memory).unwrap();
        dma(&mut memory, source, destination);
        let (_, timing) = memory.step_dma().unwrap().unwrap();
        assert_eq!(timing.total(), expected_dma);
        assert_eq!(memory.cycles(), u64::from(expected_dma));
        assert_eq!(
            cpu.step_timed(&mut memory).unwrap().code_cycles,
            expected_cpu
        );
        assert!(!memory.cpu_resume_nonsequential);
    }
}

#[test]
fn failed_dma_preserves_queue_and_clocks() {
    for (source, destination) in [(0x0e00_0000, RAM), (RAM, ROM_START)] {
        let (mut cpu, mut memory) = prepared(false, &[NOP], &[]);
        enable(&mut memory);
        cpu.step_timed(&mut memory).unwrap();
        let queue = memory.gamepak_prefetch;
        dma(&mut memory, source, destination);
        assert!(memory.step_dma().is_err());
        assert_eq!(memory.gamepak_prefetch, queue);
        assert_eq!(memory.cycles(), 0);
        assert!(!memory.cpu_resume_nonsequential);
    }
}

#[test]
fn machine_advances_prefetch_once_and_halt_progresses_while_stop_freezes() {
    let (cpu, mut memory) = prepared(false, &[0xe591_0000, NOP], &[(1, RAM)]);
    enable(&mut memory);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    machine.step().unwrap();
    assert_eq!(machine.last_timing().code_cycles, 4); // Bulk device advance must not fill queue again.
    assert_eq!(machine.cycles(), 12);

    for stop in [false, true] {
        let (mut cpu, mut memory) = prepared(false, &[NOP], &[]);
        enable(&mut memory);
        cpu.step_timed(&mut memory).unwrap();
        memory.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let queue = memory.gamepak_prefetch;
        let mut machine = Machine::new(cpu, memory);
        assert_eq!(
            machine.step().unwrap(),
            if stop {
                StepKind::StopIdle
            } else {
                StepKind::HaltIdle
            }
        );
        if stop {
            assert_eq!(machine.memory().gamepak_prefetch, queue);
            assert_eq!(machine.cycles(), 0);
        } else {
            assert_ne!(machine.memory().gamepak_prefetch, queue);
            assert!(machine.cycles() > 0);
        }
    }
}

#[test]
fn irq_source_uses_queued_code_and_bios_vector_fetches_advance_the_rom_stream() {
    let (mut cpu, mut memory) = prepared(false, &[0xe591_0000, NOP], &[(1, RAM)]);
    enable(&mut memory);
    cpu.step_timed(&mut memory).unwrap(); // Next word has two cycles of progress.
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    memory.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    memory.advance_cycles(1);
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
    assert_eq!(machine.last_timing().code_cycles, 6); // Source 4 plus vector 1+1.
    let mut queue = machine.memory().gamepak_prefetch;
    let events = machine.memory().last_cpu_timing.unwrap().events();
    let TimingEvent::Code { address, .. } = events[0] else {
        panic!("missing source")
    };
    assert_eq!(
        queue.code(address + 4, AccessWidth::Word, AccessKind::Sequential),
        4
    );
}
