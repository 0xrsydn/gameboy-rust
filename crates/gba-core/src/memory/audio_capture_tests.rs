use super::{timing_event_tests::prepared, *};
use crate::{
    audio::StereoLevel,
    io::{SOUNDBIAS, SOUNDCNT_X},
    machine::Machine,
};

#[test]
fn samples_before_store_completion_keep_old_level_and_publish_only_once() {
    let (cpu, mut memory) = prepared(false, &[0xe5810000], &[(0, 0x202), (1, SOUNDBIAS)]);
    memory.write16(SOUNDCNT_X, 0x80).unwrap();
    memory.write16(SOUNDBIAS, 0x200).unwrap();
    memory.set_audio_capture(true);
    memory.advance_cycles(505);
    let mut machine = Machine::new(cpu, memory);
    machine.step().unwrap(); // Sample at cycle 512, then bias write at same bus phase.
    let mut output = [StereoLevel::default(); 4];
    assert_eq!(machine.memory_mut().drain_audio_samples(&mut output), 1);
    assert_eq!(output[0].left, 0);
    assert_eq!(machine.memory().audio_level().left, 2);
    machine.memory_mut().advance_cycles(512);
    assert_eq!(machine.memory_mut().drain_audio_samples(&mut output), 1);
    assert_eq!(output[0].left, 2);
}

#[test]
fn failed_cpu_and_dma_steps_discard_speculative_samples_and_phase() {
    for dma in [false, true] {
        let (cpu, mut memory) = prepared(false, &[0xe8910005], &[(1, SOUNDBIAS)]); // LDM crosses unmapped 8c.
        memory.set_audio_capture(true);
        memory.advance_cycles(508);
        if dma {
            memory.write32(DMA_BASE, 0x02000000).unwrap();
            memory.write32(DMA_BASE + 4, 0x0400008c).unwrap();
            memory.write32(DMA_BASE + 8, 0x84000001).unwrap();
        }
        let before = memory.io.audio_capture.clone();
        let mut machine = Machine::new(cpu, memory);
        for _ in 0..2 {
            assert!(machine.step().is_err());
            assert_eq!(machine.memory().io.audio_capture, before);
            assert_eq!(machine.cycles(), 508);
        }
        machine.memory_mut().advance_cycles(4);
        assert_eq!(
            machine
                .memory_mut()
                .drain_audio_samples(&mut [StereoLevel::default(); 2]),
            1
        );
    }
}

#[test]
fn preflight_cannot_publish_samples_or_advance_the_sample_clock() {
    let (_, mut memory) = prepared(false, &[0xe1a00000], &[]);
    memory.set_audio_capture(true);
    memory.advance_cycles(511);
    let before = memory.io.audio_capture.clone();
    assert!(memory
        .write_words(&[(SOUNDBIAS, 0x202), (0x0400008c, 0)])
        .is_err());
    assert_eq!(memory.io.audio_capture, before);
    memory.begin_timer_step();
    memory.advance_timer_step(512);
    assert_eq!(memory.io.audio_capture, before);
    memory.discard_timer_step();
    assert_eq!(memory.io.audio_capture, before);
}
