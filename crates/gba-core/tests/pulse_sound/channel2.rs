//! Original channel 2 mapping, isolation, and shared-mixer regressions.
use super::*;

const DUTY2: u32 = 0x04000068;
const FREQUENCY2: u32 = 0x0400006c;

fn channel2() -> Memory {
    let mut bus = bus();
    bus.write16(MIX, 0x2277).unwrap();
    bus
}

fn start2(bus: &mut Memory) {
    bus.write16(DUTY2, 0xf080).unwrap();
    bus.write16(FREQUENCY2, 0x87ff).unwrap();
}

#[test]
fn channel2_register_widths_masks_and_status_do_not_alias_channel1() {
    let mut bus = channel2();
    bus.write32(DUTY2, 0xfffff0bf).unwrap(); // High halfword is unused, not frequency.
    assert_eq!(bus.read32(DUTY2).unwrap(), 0xf080);
    assert_eq!(bus.read16(DUTY).unwrap(), 0);
    bus.write8(FREQUENCY2, 255).unwrap();
    bus.write8(FREQUENCY2 + 1, 0x87).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    assert_eq!(bus.audio_level(), stereo(120));
    bus.advance_cycles(15);
    assert_eq!(bus.audio_level(), stereo(120));
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(-120));
    bus.write32(SOUNDCNT_X, 0x8f).unwrap(); // Both status bits are read-only.
    assert_eq!(bus.read32(SOUNDCNT_X).unwrap(), 0x82);
    bus.write32(FREQUENCY2, 0xffffc7ff).unwrap();
    assert_eq!(bus.read32(FREQUENCY2).unwrap(), 0x4000);
    assert_eq!(bus.read16(FREQUENCY).unwrap(), 0);
    assert_eq!(bus.read16(SWEEP).unwrap(), 0);
}

#[test]
fn channel1_sweep_and_unused_register_gaps_cannot_enable_channel2_sweep() {
    let mut dirty = channel2();
    let mut clean = channel2();
    for bus in [&mut dirty, &mut clean] {
        start2(bus); // Frequency2047: any positive sweep would overflow.
        bus.write16(SWEEP, 0x11).unwrap();
        bus.write16(DUTY, 0xf080).unwrap();
        bus.write16(FREQUENCY, 0x85dc).unwrap(); // Channel1 frequency1500 overflows on trigger.
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    }
    for address in [0x04000066, 0x0400006a, 0x0400006e] {
        dirty.write16(address, 0xffff).unwrap();
        assert_eq!(dirty.read16(address).unwrap(), 0);
    }
    for cycles in [15, 1, 31, SEQUENCE * 4, 1000000] {
        dirty.advance_cycles(cycles);
        clean.advance_cycles(cycles);
        assert_eq!(dirty.audio_level(), clean.audio_level());
        assert_eq!(dirty.read16(SOUNDCNT_X).unwrap(), 0x82);
    }
}

#[test]
fn pulse_channels_keep_independent_frequency_phase_retrigger_and_dac_state() {
    let mut bus = bus();
    bus.write16(MIX, 0x2177).unwrap(); // Channel1 right, channel2 left.
    start(&mut bus);
    bus.write16(DUTY2, 0x7080).unwrap();
    bus.write16(FREQUENCY2, 0x87fe).unwrap(); // 32 clocks per edge rather than16.
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 56,
            right: 120
        }
    );
    bus.advance_cycles(16);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 56,
            right: -120
        }
    );
    bus.write16(FREQUENCY, 0x87ff).unwrap(); // Must not restart channel2's timer.
    bus.advance_cycles(16);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: -56,
            right: -120
        }
    );
    bus.write8(DUTY + 1, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: -56,
            right: 0
        }
    );
    bus.write8(DUTY2 + 1, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
}

#[test]
fn triggers_share_sequencer_phase_but_envelopes_remain_independent() {
    let mut bus = bus();
    bus.write16(MIX, 0x2177).unwrap();
    bus.write16(DUTY, 0xc180).unwrap(); // Volume12, falling every envelope tick.
    bus.write16(FREQUENCY, 0x87ff).unwrap();
    bus.advance_cycles(SEQUENCE * 6);
    bus.write16(DUTY2, 0x3980).unwrap(); // Volume3, rising every envelope tick.
    bus.write16(FREQUENCY2, 0x87ff).unwrap();
    bus.advance_cycles(SEQUENCE * 2); // Shared step7, not eight new steps after trigger2.
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 32,
            right: 88
        }
    );
    bus.advance_cycles(SEQUENCE * 8);
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 40,
            right: 80
        }
    );
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x83);
    bus.write8(DUTY2 + 1, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    assert_eq!(bus.audio_level(), StereoLevel { left: 0, right: 80 });
}

#[test]
fn separate_length_counters_and_channel2_empty_reload_observe_shared_phase() {
    let mut bus = bus();
    bus.write16(MIX, 0).unwrap(); // Muting does not pause length.
    bus.write16(DUTY, 0xf0bf).unwrap();
    bus.write16(FREQUENCY, 0xc7ff).unwrap();
    bus.write16(DUTY2, 0xf0be).unwrap();
    bus.write16(FREQUENCY2, 0xc7ff).unwrap();
    bus.advance_cycles(SEQUENCE);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    bus.advance_cycles(SEQUENCE * 2);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(FREQUENCY2, 0xc7ff).unwrap(); // Next step3: reload length63, not64.
    bus.advance_cycles(SEQUENCE * 125);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    bus.advance_cycles(SEQUENCE);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn mixer_sums_pulses_before_rounding_and_clips_only_after_direct_sound() {
    let mut bus = bus();
    bus.write16(MIX, 0x3300).unwrap(); // Both channels, both sides, master gain1.
    bus.write16(DUTY, 0x2080).unwrap(); // +2 at duty position0.
    bus.write16(FREQUENCY, 0x8000).unwrap();
    bus.write16(DUTY2, 0x1000).unwrap(); // -1 at duty position0.
    bus.write16(FREQUENCY2, 0x8000).unwrap();
    bus.write16(SOUNDCNT_H, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(0)); // (2 - 1) >>2, not (2 >>2) + (-1 >>2).
    bus.write16(MIX, 0x2200).unwrap();
    assert_eq!(bus.audio_level(), stereo(-1));
    bus.write16(MIX, 0x3377).unwrap();
    bus.write16(SOUNDCNT_H, 0x0306).unwrap();
    start(&mut bus);
    start2(&mut bus);
    assert_eq!(bus.audio_level(), stereo(240));
    bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
    bus.write32(TIMER_BASE, 0x0080ffff).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(511));
    bus.write16(MIX, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(508));
}

#[test]
fn disabled_writes_and_channel2_dac_gate_cannot_start_playback() {
    let mut bus = channel2();
    bus.write16(SOUNDCNT_X, 0).unwrap();
    bus.write32(DUTY2, u32::MAX).unwrap();
    bus.write32(FREQUENCY2, u32::MAX).unwrap();
    assert_eq!(bus.read32(DUTY2).unwrap(), 0);
    assert_eq!(bus.read32(FREQUENCY2).unwrap(), 0);
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(DUTY2, 0x0780).unwrap();
    bus.write16(FREQUENCY2, 0x87ff).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(DUTY2, 0x0880).unwrap(); // Rising direction enables the logical DAC gate.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80); // Still needs trigger.
    bus.write16(FREQUENCY2, 0x87ff).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    bus.advance_cycles(SEQUENCE * 64); // Zero initial volume and period: active but silent.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    start(&mut bus);
    start2(&mut bus);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0);
    assert_eq!(bus.read16(DUTY).unwrap(), 0);
    assert_eq!(bus.read16(DUTY2).unwrap(), 0);
    assert_eq!(bus.audio_level(), stereo(0));
}

#[test]
fn channel2_runs_during_halt_and_freezes_during_stop() {
    for stop in [false, true] {
        let mut bus = channel2();
        start2(&mut bus);
        bus.write16(FREQUENCY2, 0x87fd).unwrap(); // 48 clocks per edge.
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        machine.step().unwrap();
        assert_eq!(
            machine.memory().audio_level(),
            stereo(if stop { 120 } else { -120 })
        );
        if stop {
            machine.memory_mut().advance_cycles(u32::MAX);
            assert_eq!(machine.cycles(), 0);
            assert_eq!(machine.memory().audio_level(), stereo(120));
        }
    }
}

#[test]
fn both_channels_keep_phase_across_large_batches_and_row_capture() {
    let mut bulk = bus();
    let mut rows = bus();
    let mut split = bus();
    rows.set_scanline_rendering(true);
    for bus in [&mut bulk, &mut rows, &mut split] {
        bus.write16(MIX, 0x2177).unwrap();
        bus.write16(SWEEP, 0x29).unwrap();
        bus.write16(DUTY, 0xa340).unwrap();
        bus.write16(FREQUENCY, 0x85ad).unwrap();
        bus.write16(DUTY2, 0x29c0).unwrap();
        bus.write16(FREQUENCY2, 0x87ab).unwrap();
    }
    for cycles in [1, 15, 32751, 262145, 77777] {
        bulk.advance_cycles(cycles);
        rows.advance_cycles(cycles);
        for _ in 0..cycles {
            split.advance_cycles(1);
        }
        assert_eq!(bulk.audio_level(), split.audio_level());
        assert_eq!(bulk.audio_level(), rows.audio_level());
        assert_eq!(
            bulk.read16(SOUNDCNT_X).unwrap(),
            split.read16(SOUNDCNT_X).unwrap()
        );
    }
    let mut bus = channel2();
    start2(&mut bus);
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x82);
    assert_eq!(bus.audio_level(), stereo(120));
}
