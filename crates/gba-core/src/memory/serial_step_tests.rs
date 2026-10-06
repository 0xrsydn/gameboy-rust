//! Serial progress belongs to the same speculative bus phases as timers/audio.
use super::{timing_event_tests::prepared, *};
use crate::{
    io::{IF, RCNT, SIOCNT, SIODATA8, SOUNDCNT_X},
    machine::{Machine, StepKind},
};

fn start(memory: &mut Memory, elapsed: u32) {
    memory.write16(SIOCNT, 0x4083).unwrap(); // 8 bits, eight cycles per bit.
    memory.advance_cycles(elapsed);
}

#[test]
fn cpu_starts_at_data_completion_and_cpu_only_execution_does_not_shift() {
    for (thumb, instruction) in [(false, 0xe1c100b0), (true, 0x8008)] {
        // STRH r0,[r1]
        let (mut cpu, mut memory) = prepared(thumb, &[instruction], &[(0, 0x4083), (1, SIOCNT)]);
        cpu.step_timed(&mut memory).unwrap();
        let started = memory.io.serial;
        assert_eq!(started.next_event_cycles(), Some(64));
        let (cpu, memory) = prepared(thumb, &[instruction], &[(0, 0x4083), (1, SIOCNT)]);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.memory().io.serial, started); // No preceding fetch/data cycles charged to the new request.
        assert_eq!(machine.last_timing().data_cycles, 1);
        machine.memory_mut().advance_cycles(63);
        assert_eq!(machine.memory().read8(SIODATA8).unwrap(), 0x7f);
        machine.memory_mut().advance_cycles(1);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
    }
}

#[test]
fn arm_and_thumb_loads_sample_before_the_trailing_internal_cycle() {
    for (thumb, instruction, elapsed) in [(false, 0xe1d100b0, 56), (true, 0x8808, 59)] {
        let (cpu, mut memory) = prepared(thumb, &[instruction], &[(1, SIOCNT)]);
        start(&mut memory, elapsed);
        let mut machine = Machine::new(cpu, memory);
        machine.step().unwrap();
        assert_eq!(machine.cpu().registers()[0], 0x4087);
        assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x4007);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
        assert_eq!(machine.cycles(), 64);
    }
}

#[test]
fn if_acknowledgement_at_the_completion_phase_clears_serial_but_not_other_sources() {
    let (cpu, mut memory) = prepared(false, &[0xe1c100b0], &[(0, 0x80), (1, IF)]);
    start(&mut memory, 57);
    memory.write16(crate::io::KEYCNT, 0x4001).unwrap();
    memory.set_buttons(crate::input::Buttons::from_bits(1));
    assert_eq!(memory.read16(IF).unwrap(), 0x1000);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap(); // Fetch six cycles + one data cycle: complete, then acknowledge.
    assert_eq!(machine.cycles(), 64);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x1000);
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x4007);
}

#[test]
fn single_and_block_store_validation_uses_completion_phase_and_does_not_double_advance() {
    for instruction in [0xe5810000, 0xe8a10001] {
        // STR and STMIA r1!,{r0}
        for elapsed in [56, 57] {
            let (cpu, mut memory) =
                prepared(false, &[instruction], &[(0, 0x00204081), (1, SIOCNT)]);
            start(&mut memory, elapsed);
            let serial = memory.io.serial;
            let mut machine = Machine::new(cpu, memory);
            let cpu = machine.cpu().clone();
            if elapsed == 57 {
                machine.step().unwrap(); // Previous request completes at this write's phase.
                assert_eq!(machine.memory().read16(IF).unwrap(), 0x80);
                assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x4085);
                assert_eq!(machine.memory().read8(SIODATA8).unwrap(), 0x20);
                assert_eq!(machine.memory().io.serial.next_event_cycles(), Some(512));
                assert_eq!(machine.cycles(), 64);
            } else {
                let error = machine.step().unwrap_err();
                assert!(error.to_string().contains("serial reconfiguration"));
                assert_eq!(machine.step(), Err(error));
                assert_eq!(machine.cpu(), &cpu);
                assert_eq!(machine.cycles(), u64::from(elapsed));
                assert_eq!(machine.memory().io.serial, serial);
                assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            }
            assert!(machine.memory().timer_step.get().is_none());
            assert!(machine.memory().cpu_timing.get().is_none());
        }
    }
}

#[test]
fn later_block_failure_restores_completion_and_partial_serial_writes() {
    let (cpu, mut memory) = prepared(
        false,
        &[0xe8a10005],
        &[(0, 0x00204081), (1, SIOCNT), (2, 0)],
    );
    start(&mut memory, 57);
    let serial = memory.io.serial;
    let mut machine = Machine::new(cpu, memory);
    let cpu = machine.cpu().clone();
    for _ in 0..2 {
        assert!(machine
            .step()
            .unwrap_err()
            .to_string()
            .contains("0x0400012c"));
        assert_eq!(machine.memory().io.serial, serial);
        assert_eq!(machine.memory().read16(IF).unwrap(), 0);
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.cycles(), 57);
    }
}

#[test]
fn dma_source_observes_old_busy_then_destination_phase_commits_completion() {
    let (cpu, mut memory) = prepared(false, &[0xe1a00000], &[]);
    start(&mut memory, 60);
    memory.write32(DMA_BASE, SIOCNT).unwrap();
    memory.write32(DMA_BASE + 4, 0x02000000).unwrap();
    memory.write32(DMA_BASE + 8, 0xc4000001).unwrap();
    let mut machine = Machine::new(cpu, memory);
    assert_eq!(machine.step().unwrap(), StepKind::Dma { channel: 0 });
    assert_eq!(machine.memory().read32(0x02000000).unwrap(), 0x007f4087);
    assert_eq!(machine.memory().read16(SIOCNT).unwrap(), 0x4007);
    assert_eq!(machine.memory().read16(IF).unwrap(), 0x180); // Both sources survive their separate commits.
    assert_eq!(machine.cycles(), 69);
}

#[test]
fn dma_start_excludes_source_cycles_and_failed_destination_discards_serial_progress() {
    for fail in [false, true] {
        let (cpu, mut memory) = prepared(false, &[0xe1a00000], &[]);
        if fail {
            start(&mut memory, 60);
            memory.write16(SOUNDCNT_X, 0x80).unwrap();
            memory.write16(0x04000070, 0xa0).unwrap(); // Unsupported two-bank wave playback.
        }
        memory
            .write32(0x02000000, if fail { 0x8000 } else { 0x4083 })
            .unwrap();
        memory.write32(DMA_BASE, 0x02000000).unwrap();
        memory
            .write32(DMA_BASE + 4, if fail { 0x04000074 } else { SIOCNT })
            .unwrap();
        memory.write32(DMA_BASE + 8, 0x84000001).unwrap();
        let serial = memory.io.serial;
        let mut machine = Machine::new(cpu, memory);
        if fail {
            let cpu = machine.cpu().clone();
            let error = machine.step().unwrap_err();
            assert_eq!(machine.step(), Err(error));
            assert_eq!(machine.memory().io.serial, serial);
            assert_eq!(machine.memory().read16(IF).unwrap(), 0);
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), 60);
        } else {
            machine.step().unwrap();
            assert_eq!(machine.cycles(), 9);
            assert_eq!(machine.memory().io.serial.next_event_cycles(), Some(64));
        }
    }
}

#[test]
fn batch_validation_carries_control_changes_without_committing_earlier_ram_on_error() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    start(&mut memory, 8);
    let serial = memory.io.serial;
    assert!(memory
        .write_words(&[(0x02000000, 0x12345678), (SIOCNT, 0x00aa4083)])
        .is_err());
    assert_eq!(memory.read32(0x02000000).unwrap(), 0);
    assert_eq!(memory.io.serial, serial);
    memory
        .write_words(&[(SIOCNT, 0), (SIOCNT, 0x00aa4083)])
        .unwrap();
    assert_eq!(memory.read8(SIODATA8).unwrap(), 0xaa);
    assert_eq!(memory.io.serial.next_event_cycles(), Some(64));
}

#[test]
fn staged_unmapped_pin_reads_do_not_fall_back_to_old_committed_gpio_state() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    memory.write16(RCNT, 0x8000).unwrap();
    memory.begin_timer_step();
    memory.write16(RCNT, 0).unwrap();
    assert_eq!(memory.read8(RCNT), Err(MemoryError::Unmapped(RCNT)));
    memory.discard_timer_step();
    assert_eq!(memory.read16(RCNT).unwrap(), 0x800f);
}

#[test]
fn capture_splits_and_cpu_clock_commits_do_not_duplicate_serial_edges() {
    let (cpu, mut plain) = prepared(false, &[0xe1a00000; 100], &[]);
    let (_, mut captured) = prepared(false, &[0xe1a00000; 100], &[]);
    plain.advance_cycles(950);
    captured.advance_cycles(950);
    captured.set_scanline_rendering(true);
    start(&mut plain, 0);
    start(&mut captured, 0);
    let mut plain = Machine::new(cpu.clone(), plain);
    let mut captured = Machine::new(cpu, captured);
    for _ in 0..20 {
        plain.step().unwrap();
        captured.step().unwrap();
        assert_eq!(plain.memory().io.serial, captured.memory().io.serial);
        assert_eq!(plain.memory().read16(IF), captured.memory().read16(IF));
        assert_eq!(plain.cycles(), captured.cycles());
    }
    assert_eq!(plain.memory().read8(SIODATA8).unwrap(), 0xff);
}
