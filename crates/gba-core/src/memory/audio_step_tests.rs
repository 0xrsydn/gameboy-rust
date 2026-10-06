//! Original checks of audio bus phases and transactional timer/audio ownership.
use super::{timing_event_tests::prepared, *};
use crate::{
    io::{FIFO_A, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE},
    machine::Machine,
};

fn sound(memory: &mut Memory, period: u16) {
    memory.write16(SOUNDCNT_X, 0x80).unwrap();
    memory.write16(SOUNDBIAS, 0x200).unwrap();
    memory.write16(SOUNDCNT_H, 0x0304).unwrap();
    memory
        .write32(
            TIMER_BASE,
            0x00800000 | u32::from(0u16.wrapping_sub(period)),
        )
        .unwrap();
}

#[test]
fn fifo_store_becomes_visible_after_its_bus_completion_not_at_instruction_start() {
    let (cpu, mut memory) = prepared(
        false,
        &[0xe5810000, 0xe1a00000],
        &[(0, 0x04030201), (1, FIFO_A)],
    );
    sound(&mut memory, 1);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.cycles(), 7);
    assert_eq!(machine.memory().audio_level().left, 0); // Earlier seven empty overflows.
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.memory().audio_level().left, 4);
}

#[test]
fn master_enable_store_does_not_replay_earlier_timer_edges() {
    let (cpu, mut memory) = prepared(false, &[0xe5810000], &[(0, 0x80), (1, SOUNDCNT_X)]);
    sound(&mut memory, 1);
    memory.write16(SOUNDCNT_X, 0).unwrap();
    memory.write32(FIFO_A, 0x04030201).unwrap();
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.memory().audio_level().left, 0);
    machine.memory_mut().advance_cycles(1);
    assert_eq!(machine.memory().audio_level().left, 4);
}

#[test]
fn failed_cpu_load_rolls_back_timer_audio_and_pending_refill_requests() {
    let (cpu, mut memory) = prepared(false, &[0xe8910005], &[(1, SOUNDBIAS)]); // LDM: bias then unmapped 8c.
    sound(&mut memory, 1);
    memory.write32(FIFO_A, 0x04030201).unwrap();
    let audio = memory.io.audio;
    let mut machine = Machine::new(cpu, memory);
    let cpu = machine.cpu().clone();
    for _ in 0..2 {
        assert!(machine.step().is_err());
        assert_eq!(machine.memory().io.audio, audio);
        assert_eq!(machine.cycles(), 0);
        assert_eq!(machine.cpu(), &cpu);
        assert!(machine.memory().timer_step.get().is_none());
    }
}

#[test]
fn failed_dma_source_and_destination_restore_audio_after_speculative_timer_edges() {
    for (source, destination, word) in [(FIFO_A, 0x02001000, 0), (0x02000000, 0x04000064, 0x8000)] {
        let (cpu, mut memory) = prepared(false, &[0xe1a00000], &[]);
        sound(&mut memory, 1);
        memory.write32(FIFO_A, 0x04030201).unwrap();
        memory.write32(0x02000000, word).unwrap();
        memory.write32(DMA_BASE, source).unwrap();
        memory.write32(DMA_BASE + 4, destination).unwrap();
        memory.write32(DMA_BASE + 8, 0x84000001).unwrap();
        let audio = memory.io.audio;
        let mut machine = Machine::new(cpu, memory);
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.memory().io.audio, audio);
            assert_eq!(machine.cycles(), 0);
        }
    }
}

#[test]
fn batch_validation_simulates_sound_enable_and_disable_before_committing_any_write() {
    let mut memory = Memory::new(vec![]).unwrap();
    let original = memory.io.audio;
    assert!(memory
        .write_words(&[
            (0x02000000, 0x11223344),
            (SOUNDCNT_X, 0x80),
            (0x04000064, 0x8000),
        ])
        .is_err());
    assert_eq!(memory.read32(0x02000000).unwrap(), 0);
    assert_eq!(memory.io.audio, original);
    memory
        .write_words(&[(SOUNDCNT_X, 0x80), (SOUNDCNT_X, 0), (0x04000064, 0x8000)])
        .unwrap();
    assert_eq!(memory.read16(0x04000064).unwrap(), 0);
    assert_eq!(memory.read16(SOUNDCNT_X).unwrap(), 0);
}
