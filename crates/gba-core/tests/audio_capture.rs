//! Original fixed-rate sample probes; no host device or external recordings.
use gba_core::{
    audio::{StereoLevel, AUDIO_QUEUE_CAPACITY, AUDIO_SAMPLE_RATE},
    io::{FIFO_A, HALTCNT, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE},
    memory::Memory,
};
fn bus() -> Memory {
    let mut bus = Memory::new(vec![]).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(SOUNDCNT_H, 0x0304).unwrap();
    bus
}
fn drain(bus: &mut Memory) -> Vec<StereoLevel> {
    let mut output = vec![StereoLevel::default(); AUDIO_QUEUE_CAPACITY];
    let count = bus.drain_audio_samples(&mut output);
    output.truncate(count);
    output
}
#[test]
fn exact_rate_phase_toggle_and_drain_order() {
    assert_eq!(AUDIO_SAMPLE_RATE * 512, 16777216);
    let mut bus = bus();
    bus.advance_cycles(100);
    assert!(drain(&mut bus).is_empty());
    bus.set_audio_capture(true);
    bus.advance_cycles(511);
    bus.set_audio_capture(true); // Idempotent; no phase reset.
    assert!(drain(&mut bus).is_empty());
    bus.advance_cycles(1);
    bus.write16(SOUNDBIAS, 0x202).unwrap();
    bus.advance_cycles(512);
    let mut one = [StereoLevel::default(); 1];
    assert_eq!(bus.drain_audio_samples(&mut one), 1);
    assert_eq!(one[0].left, 0);
    assert_eq!(bus.drain_audio_samples(&mut one), 1);
    assert_eq!(one[0].left, 2);
    assert_eq!(bus.drain_audio_samples(&mut one), 0);
    bus.advance_cycles(511);
    bus.set_audio_capture(false);
    bus.set_audio_capture(true);
    bus.advance_cycles(1);
    assert!(drain(&mut bus).is_empty());
}
#[test]
fn timer_overflow_at_sample_boundary_precedes_sample_and_later_stores() {
    let mut bus = bus();
    bus.write32(FIFO_A, 0x04030201).unwrap();
    bus.write32(TIMER_BASE, 0x0080fe00).unwrap(); // 512-clock timer.
    bus.set_audio_capture(true);
    bus.advance_cycles(4 * 512);
    assert_eq!(
        drain(&mut bus).iter().map(|s| s.left).collect::<Vec<_>>(),
        [4, 8, 12, 16]
    );
    bus.advance_cycles(512);
    assert_eq!(drain(&mut bus)[0].left, 0);
}
#[test]
fn psg_sample_clock_crosses_many_oscillator_edges_and_respects_halt_stop() {
    let mut bus = bus();
    bus.write16(SOUNDCNT_H, 2).unwrap();
    bus.write16(0x04000080, 0x1177).unwrap();
    bus.write16(0x04000062, 0xf080).unwrap();
    bus.write16(0x04000064, 0x87e0).unwrap(); // One duty step every 512 cycles.
    bus.set_audio_capture(true);
    bus.write8(HALTCNT, 0).unwrap();
    bus.advance_cycles(8 * 512);
    assert_eq!(
        drain(&mut bus).iter().map(|s| s.left).collect::<Vec<_>>(),
        [-120, -120, -120, -120, 120, 120, 120, 120]
    );
    bus.write8(HALTCNT, 0x80).unwrap();
    bus.advance_cycles(8192);
    assert!(drain(&mut bus).is_empty());
}
#[test]
fn bulk_split_capture_and_no_capture_keep_hardware_state_and_samples_equivalent() {
    let mut a = bus();
    let mut b = bus();
    let mut c = bus();
    for bus in [&mut a, &mut b, &mut c] {
        bus.write32(FIFO_A, 0x807f01ff).unwrap();
        bus.write32(TIMER_BASE, 0x0080ff9c).unwrap();
    }
    a.set_audio_capture(true);
    b.set_audio_capture(true);
    b.set_scanline_rendering(true);
    a.advance_cycles(100003);
    c.advance_cycles(100003);
    for _ in 0..100003 {
        b.advance_cycles(1);
    }
    assert_eq!(drain(&mut a), drain(&mut b));
    assert_eq!(a.audio_level(), c.audio_level());
    assert_eq!(a.read32(TIMER_BASE), c.read32(TIMER_BASE));
    assert_eq!(a.audio_dropped_samples(), 0);
}
#[test]
fn bounded_overflow_counts_drops_and_keeps_earliest_frames_without_clock_stall() {
    let mut bus = bus();
    bus.set_audio_capture(true);
    bus.advance_cycles(u32::MAX);
    assert_eq!(
        bus.audio_dropped_samples(),
        u64::from(u32::MAX) / 512 - AUDIO_QUEUE_CAPACITY as u64
    );
    assert_eq!(drain(&mut bus).len(), AUDIO_QUEUE_CAPACITY);
    bus.advance_cycles(1);
    assert_eq!(drain(&mut bus).len(), 1); // Overflow did not lose the phase.
    bus.set_audio_capture(false);
    assert_eq!(bus.audio_dropped_samples(), 0);
}
