//! Original checks of timer transaction ownership and bus-completion sampling.
use super::{timing_event_tests::prepared, *};
use crate::{
    cpu::Cpu,
    display::HBLANK_START,
    dma::DMA_STRIDE,
    io::{DISPSTAT, HALTCNT, IE, IF, IME, TIMER_BASE, WAITCNT},
    machine::{Machine, StepKind},
};

const NOP: u32 = 0xe1a0_0000;
const RAM: u32 = 0x0300_1000;

fn start(memory: &mut Memory, index: u32, reload: u16, control: u16) {
    memory
        .write32(
            TIMER_BASE + index * 4,
            u32::from(reload) | u32::from(control) << 16,
        )
        .unwrap();
}

fn dma(memory: &mut Memory, source: u32, destination: u32, count: u16) {
    let base = DMA_BASE + 3 * DMA_STRIDE;
    memory.write32(base, source).unwrap();
    memory.write32(base + 4, destination).unwrap();
    memory
        .write32(base + 8, 0x8400_0000 | u32::from(count))
        .unwrap();
}

#[test]
fn arm_and_thumb_timer_loads_observe_source_and_data_but_not_trailing_internal_cycles() {
    for (thumb, instruction, sample, final_count) in
        [(false, 0xe591_0000, 7, 8), (true, 0x6808, 4, 5)]
    {
        let (cpu, mut memory) = prepared(thumb, &[instruction], &[(1, TIMER_BASE)]);
        start(&mut memory, 0, 0, 0x80);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], 0x0080_0000 | sample);
        assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), final_count);
        assert_eq!(machine.cycles(), u64::from(final_count));
        assert!(machine.memory().timer_step.get().is_none());
    }
}

#[test]
fn byte_halfword_and_word_lanes_share_one_completed_access() {
    for (instruction, address, expected) in [
        (0xe5d1_0000, TIMER_BASE, 6),
        (0xe5d1_0000, TIMER_BASE + 1, 1),
        (0xe1d1_00b0, TIMER_BASE, 0x106),
        (0xe591_0000, TIMER_BASE, 0x0080_0106),
    ] {
        let (cpu, mut memory) = prepared(false, &[instruction], &[(1, address)]);
        start(&mut memory, 0, 0xff, 0x80);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], expected);
        assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0x107);
        assert_eq!(machine.cycles(), 8);
    }
}

#[test]
fn starting_and_stopping_timers_take_effect_after_the_store_bus_cycle() {
    for running in [false, true] {
        let control = if running { 0 } else { 0x0080_0000 };
        let (cpu, mut memory) =
            prepared(false, &[0xe581_0000, NOP], &[(0, control), (1, TIMER_BASE)]);
        if running {
            start(&mut memory, 0, 0, 0x80);
        }
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.cycles(), 7);
        assert_eq!(
            machine.memory().read16(TIMER_BASE).unwrap(),
            if running { 7 } else { 0 }
        );
        machine.step().unwrap(); // Store left N: this ROM fetch costs eight cycles.
        assert_eq!(
            machine.memory().read16(TIMER_BASE).unwrap(),
            if running { 7 } else { 8 }
        );
        assert_eq!(machine.cycles(), 15);
    }
}

#[test]
fn block_timer_accesses_keep_distinct_word_phases() {
    let (cpu, mut memory) = prepared(false, &[0xe891_0005], &[(1, TIMER_BASE)]); // LDM r1,{r0,r2}
    start(&mut memory, 0, 0, 0x80);
    start(&mut memory, 1, 0, 0x80);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[0], 0x0080_0007);
    assert_eq!(machine.cpu().registers()[2], 0x0080_0008);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 9);
    assert_eq!(machine.memory().read16(TIMER_BASE + 4).unwrap(), 9);

    let (cpu, memory) = prepared(
        false,
        &[0xe880_0006],
        &[(0, TIMER_BASE), (1, 0x0080_0000), (2, 0x0080_0000)],
    );
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 1);
    assert_eq!(machine.memory().read16(TIMER_BASE + 4).unwrap(), 0);
    assert_eq!(machine.cycles(), 8);
}

#[test]
fn reload_changes_preserve_pre_write_overflows_and_prescaler_phase() {
    // Overflow during source fetch uses the old reload, not the value stored afterward.
    let (cpu, mut memory) = prepared(false, &[0xe1c1_00b0], &[(0, 0x1234), (1, TIMER_BASE)]);
    start(&mut memory, 0, 0xfffa, 0xc0);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0xfffb);
    assert_eq!(machine.memory().read16(IF).unwrap(), 8);
    machine.memory_mut().advance_cycles(5);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0x1234);

    // A read crossing a prescaler edge must retain the remainder for the next step.
    let (cpu, mut memory) = prepared(false, &[0xe591_0000], &[(1, TIMER_BASE)]);
    start(&mut memory, 0, 0, 0x81);
    memory.advance_cycles(60);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[0], 0x0081_0001);
    machine.memory_mut().advance_cycles(59);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 1);
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 2);
}

#[test]
fn cascades_and_timer_irq_bits_are_visible_at_the_read_phase() {
    let (cpu, mut memory) = prepared(false, &[0xe591_0000], &[(1, TIMER_BASE + 4)]);
    start(&mut memory, 0, 0xfffe, 0xc0);
    start(&mut memory, 1, 0xfffe, 0xc4);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap(); // Source+data=7 gives three timer0 pulses and one timer1 overflow.
    assert_eq!(machine.cpu().registers()[0], 0x00c4_ffff);
    assert_eq!(machine.memory().read16(TIMER_BASE + 4).unwrap(), 0xfffe);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x18);

    let (cpu, mut memory) = prepared(false, &[0xe591_0000], &[(1, IE)]);
    start(&mut memory, 0, 0xfffa, 0xc0);
    memory.write16(IE, 8).unwrap();
    memory.write16(IME, 1).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Instruction);
    assert_eq!(machine.cpu().registers()[0], 0x0008_0008); // Word IE+IF, coherent timer flag.
    assert_eq!(machine.step().unwrap(), StepKind::IrqEntry);
}

#[test]
fn if_acknowledgement_clears_earlier_timer_events_but_not_later_ones() {
    for (instruction, reload, expected) in [(0xe1c1_00b0, 0xfffa, 0), (0xe141_0092, 0xffff, 8)] {
        let (cpu, mut memory) = prepared(false, &[instruction], &[(0, 8), (1, IF), (2, 8)]);
        start(&mut memory, 0, reload, 0xc0);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.memory().read16(IF).unwrap(), expected);
        if instruction == 0xe141_0092 {
            // SWPB acknowledges at the second data access; its final I cycle raises IF again.
            assert_eq!(machine.cpu().registers()[0], 8);
        }
    }
}

#[test]
fn failed_instructions_discard_partial_timer_progress_and_pending_irqs() {
    for (instruction, base) in [
        (0xe591_0000, 0x0e00_0000),
        (0xe891_0005, TIMER_BASE + 12),
        (0xe881_0005, TIMER_BASE + 12),
        (0xffff_ffff, 0),
    ] {
        let (cpu, mut memory) = prepared(false, &[instruction], &[(1, base)]);
        start(&mut memory, 0, 0xfffc, 0xc0);
        let before = memory.io.timer_step();
        let cpu_before = cpu.clone();
        let mut machine = Machine::new(cpu, memory);
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.memory().io.timer_step(), before);
            assert_eq!(machine.cpu(), &cpu_before);
            assert_eq!(machine.cycles(), 0);
            assert!(machine.memory().timer_step.get().is_none());
            assert!(machine.memory().cpu_timing.get().is_none());
        }
    }
    // A missing current instruction fails before CpuTiming starts, but must still discard TimerStep.
    let mut memory = Memory::new(Vec::new()).unwrap();
    start(&mut memory, 0, 0xffff, 0xc0);
    let mut machine = Machine::new(Cpu::new(ROM_START), memory);
    assert!(machine.step().is_err());
    assert!(machine.memory().timer_step.get().is_none());
    assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 0xffff);
}

#[test]
fn cpu_only_apis_and_host_reads_do_not_advance_timer_devices() {
    for prefetch in [0, 0x4000] {
        for timed in [false, true] {
            let (mut cpu, mut memory) = prepared(false, &[0xe591_0000], &[(1, TIMER_BASE)]);
            memory.write16(WAITCNT, prefetch).unwrap();
            start(&mut memory, 0, 123, 0x80);
            let before = memory.io.timer_step();
            if timed {
                cpu.step_timed(&mut memory).unwrap();
            } else {
                cpu.step(&mut memory).unwrap();
            }
            assert_eq!(cpu.registers()[0], 0x0080_007b);
            assert_eq!(memory.read16(TIMER_BASE).unwrap(), 123);
            assert_eq!(memory.io.timer_step(), before);
            assert_eq!(memory.cycles(), 0);
        }
    }
}

#[test]
fn dma_samples_timers_after_startup_and_source_and_writes_after_destination() {
    let mut memory = Memory::new(Vec::new()).unwrap();
    start(&mut memory, 0, 0, 0x80);
    dma(&mut memory, TIMER_BASE, RAM, 1);
    memory.step_dma().unwrap().unwrap();
    assert_eq!(memory.read32(RAM).unwrap(), 0x0080_0003);
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 4);
    assert_eq!(memory.cycles(), 4);

    let mut memory = Memory::new(Vec::new()).unwrap();
    memory.write32(RAM, 0x0080_0000).unwrap();
    memory.write32(RAM + 4, 0x0080_0000).unwrap();
    dma(&mut memory, RAM, TIMER_BASE, 2);
    memory.step_dma().unwrap().unwrap();
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 0);
    memory.step_dma().unwrap().unwrap();
    assert_eq!(memory.read16(TIMER_BASE).unwrap(), 2);
    assert_eq!(memory.read16(TIMER_BASE + 4).unwrap(), 0);
    assert_eq!(memory.cycles(), 6);
}

#[test]
fn dma_failures_preserve_timers_and_if_and_clear_the_transaction() {
    for (source, destination) in [(0x0e00_0000, RAM), (TIMER_BASE, ROM_START)] {
        let mut memory = Memory::new(Vec::new()).unwrap();
        start(&mut memory, 0, 0xffff, 0xc0);
        let before = memory.io.timer_step();
        dma(&mut memory, source, destination, 1);
        assert!(memory.step_dma().is_err());
        assert_eq!(memory.io.timer_step(), before);
        assert!(memory.timer_step.get().is_none());
        assert_eq!(memory.cycles(), 0);
    }
}

#[test]
fn display_capture_splits_do_not_advance_committed_timers_twice() {
    for capture in [false, true] {
        let (cpu, mut memory) = prepared(false, &[0xe891_007d], &[(1, 0x0200_1000)]);
        memory.set_scanline_rendering(capture);
        memory.advance_cycles(HBLANK_START - 2);
        start(&mut memory, 0, 0, 0x80);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap(); // Source 6, six EWRAM words 36, internal 1.
        assert_eq!(machine.last_timing().total(), 43);
        assert_eq!(machine.memory().read16(TIMER_BASE).unwrap(), 43);
        assert_eq!(machine.cycles(), u64::from(HBLANK_START - 2 + 43));
    }
}

#[test]
fn timer_flag_commit_preserves_other_irq_sources() {
    for (instruction, expected) in [(0xe1d1_00b0, 0xa), (0xe1c1_00b0, 2)] {
        let (cpu, mut memory) = prepared(false, &[instruction], &[(0, 8), (1, IF)]);
        memory.write16(DISPSTAT, 0x10).unwrap();
        memory.advance_cycles(HBLANK_START);
        assert_eq!(memory.read16(IF).unwrap(), 2);
        start(&mut memory, 0, 0xfffa, 0xc0);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.memory().read16(IF).unwrap(), expected);
        if instruction == 0xe1d1_00b0 {
            assert_eq!(machine.cpu().registers()[0], 0xa);
        }
    }
}

#[test]
fn bios_power_store_pays_its_cycles_and_new_timer_irq_wakes_only_halt() {
    for stop in [false, true] {
        let (mut cpu, mut memory) = prepared(
            false,
            &[0xe12f_ff1c],
            &[(0, if stop { 0x80 } else { 0 }), (1, HALTCNT), (12, 0x100)],
        );
        memory.bios.as_mut().unwrap()[0x100..0x104].copy_from_slice(&0xe5c1_0000_u32.to_le_bytes());
        cpu.step(&mut memory).unwrap(); // BX into original BIOS STRB r0,[r1].
        start(&mut memory, 0, 0xffff, 0xc0);
        memory.write16(IE, 8).unwrap();
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.cycles(), 2);
        assert_eq!(machine.memory().read16(IF).unwrap(), 8);
        assert!(!machine.halted());
        assert_eq!(machine.stopped(), stop);
        if stop {
            let before = machine.memory().io.timer_step();
            assert_eq!(machine.step().unwrap(), StepKind::StopIdle);
            assert_eq!(machine.memory().io.timer_step(), before);
            assert_eq!(machine.cycles(), 2);
        } else {
            assert_eq!(machine.step().unwrap(), StepKind::Instruction);
        }
    }
}

#[test]
fn halt_and_stop_keep_their_clock_ownership() {
    for stop in [false, true] {
        let mut memory = Memory::new(Vec::new()).unwrap();
        start(&mut memory, 0, 0, 0x80);
        memory.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), memory);
        assert_eq!(
            machine.step().unwrap(),
            if stop {
                StepKind::StopIdle
            } else {
                StepKind::HaltIdle
            }
        );
        assert_eq!(
            u64::from(machine.memory().read16(TIMER_BASE).unwrap()),
            machine.cycles()
        );
        assert!(machine.memory().timer_step.get().is_none());
    }
}
