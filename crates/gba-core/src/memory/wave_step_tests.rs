//! Original wave clocks, RAM rotation, and access-phase rollback probes.
use super::{timing_event_tests::prepared, *};
use crate::{
    io::{SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, WAVE_RAM},
    machine::{Machine, StepKind},
};

fn setup(bus: &mut Memory) {
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(SOUNDCNT_H, 2).unwrap();
    bus.write16(0x04000080, 0x4477).unwrap();
    bus.write16(0x04000070, 0x40).unwrap();
    bus.write32(WAVE_RAM, 0x123456f0).unwrap(); // Bank 0, high nibble first.
    bus.write16(0x04000070, 0x80).unwrap();
    bus.write16(0x04000072, 0x2000).unwrap();
}

#[test]
fn arm_thumb_and_dma_triggers_start_after_the_store_bus_phase() {
    for kind in 0..3 {
        let (cpu, mut memory) = prepared(
            kind == 1,
            &[if kind == 1 { 0x6008 } else { 0xe5810000 }],
            &[(0, 0x87ff), (1, 0x04000074)],
        );
        setup(&mut memory);
        if kind == 2 {
            memory.write32(0x02000000, 0x87ff).unwrap();
            memory.write32(DMA_BASE, 0x02000000).unwrap();
            memory.write32(DMA_BASE + 4, 0x04000074).unwrap();
            memory.write32(DMA_BASE + 8, 0x84000001).unwrap();
        }
        let mut machine = Machine::new(cpu, memory);
        assert_eq!(
            machine.step().unwrap(),
            if kind == 2 {
                StepKind::Dma { channel: 0 }
            } else {
                StepKind::Instruction
            }
        );
        assert_eq!(machine.memory().read16(SOUNDCNT_X).unwrap(), 0x84);
        machine.memory_mut().advance_cycles(7);
        assert_eq!(machine.memory().audio_level().left, 0);
        machine.memory_mut().advance_cycles(1);
        assert_eq!(machine.memory().audio_level().left, 112);
    }
}

#[test]
fn length_expiry_at_load_bus_completion_is_visible_in_status() {
    let (cpu, mut memory) = prepared(false, &[0xe5910000], &[(1, SOUNDCNT_X)]);
    setup(&mut memory);
    memory.write16(0x04000072, 0x20ff).unwrap();
    memory.write16(0x04000074, 0xc7ff).unwrap();
    memory.advance_cycles(32768 - 7);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap();
    assert_eq!(machine.cpu().registers()[0], 0x80);
    assert_eq!(machine.memory().audio_level().left, 0);
}

#[test]
fn failed_block_access_rolls_back_rotated_ram_samples_configuration_and_clocks() {
    for store in [false, true] {
        let (cpu, mut memory) = if store {
            prepared(
                false,
                &[0xe8a0003e],
                &[
                    (0, 0x04000070),
                    (1, 0x20000080),
                    (2, 0x87ff),
                    (3, 0),
                    (4, 0),
                    (5, 0x00030000),
                ],
            )
        } else {
            prepared(false, &[0xe8910005], &[(1, SOUNDBIAS)])
        };
        setup(&mut memory);
        memory.write16(0x04000074, 0x87ff).unwrap();
        memory.advance_cycles(7);
        let audio = memory.io.audio;
        let mut machine = Machine::new(cpu, memory);
        let cpu = machine.cpu().clone();
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.memory().io.audio, audio);
            assert_eq!(machine.cpu(), &cpu);
            assert_eq!(machine.cycles(), 7);
        }
    }
}

#[test]
fn staged_bank_switch_reads_the_current_rotated_ram_not_committed_storage() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    setup(&mut memory);
    memory.write16(0x04000074, 0x87ff).unwrap();
    memory.advance_cycles(8);
    let audio = memory.io.audio;
    memory.begin_timer_step();
    memory.write16(0x04000070, 0x40).unwrap(); // Stop and expose played bank.
    assert_eq!(memory.read32(WAVE_RAM).unwrap(), 0x20416305);
    memory.write32(WAVE_RAM, 0xaabbccdd).unwrap();
    assert_eq!(memory.read32(WAVE_RAM).unwrap(), 0xaabbccdd);
    memory.discard_timer_step();
    assert_eq!(memory.io.audio, audio);
}
