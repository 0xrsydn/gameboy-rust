//! Original public-bus tests for PSG pulse channels; no game code or audio assets.
#[path = "pulse_sound/channel2.rs"]
mod channel2;
use gba_core::{
    audio::StereoLevel,
    cpu::Cpu,
    io::{FIFO_A, HALTCNT, IF, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE},
    machine::Machine,
    memory::{Memory, ROM_START},
};

const SWEEP: u32 = 0x04000060;
const DUTY: u32 = 0x04000062;
const FREQUENCY: u32 = 0x04000064;
const MIX: u32 = 0x04000080;
const SEQUENCE: u32 = 32768;

fn bus() -> Memory {
    let mut bus = Memory::new(0xeafffffeu32.to_le_bytes().to_vec()).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(MIX, 0x1177).unwrap(); // Channel 1, both sides, maximum volume.
    bus.write16(SOUNDCNT_H, 2).unwrap(); // PSG 100%.
    bus
}

fn start(bus: &mut Memory) {
    bus.write16(DUTY, 0xf080).unwrap(); // 50% duty, volume15, no envelope, length64.
    bus.write16(FREQUENCY, 0x87ff).unwrap(); // Frequency2047, trigger; length disabled.
}

fn stereo(value: i16) -> StereoLevel {
    StereoLevel {
        left: value,
        right: value,
    }
}

#[test]
fn register_widths_masks_status_and_wave_edges_without_general_timers() {
    let mut bus = bus();
    bus.write32(SWEEP, 0xf0bf0080).unwrap(); // Sweep bit7 ignored; duty50%, length1.
    assert_eq!(bus.read32(SWEEP).unwrap(), 0xf0800000);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write8(FREQUENCY, 255).unwrap();
    bus.write8(FREQUENCY + 1, 0x87).unwrap();
    assert_eq!(bus.read32(FREQUENCY).unwrap(), 0);
    assert_eq!(bus.read32(SOUNDCNT_X).unwrap(), 0x81);
    bus.write16(SOUNDCNT_X, 0x80).unwrap(); // Status cannot be cleared by a write.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    assert_eq!(bus.audio_level(), stereo(120));
    bus.advance_cycles(15);
    assert_eq!(bus.audio_level(), stereo(120));
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(-120));
    bus.advance_cycles(64);
    assert_eq!(bus.audio_level(), stereo(120));
    assert_eq!(bus.read16(IF).unwrap(), 0); // PSG clocks do not generate IRQs.
}

#[test]
fn psg_stereo_volume_ratios_and_direct_sound_share_bias_and_clipping() {
    let mut bus = bus();
    start(&mut bus);
    for (ratio, expected) in [(0, 30), (1, 60), (2, 120)] {
        bus.write16(SOUNDCNT_H, ratio).unwrap();
        assert_eq!(bus.audio_level(), stereo(expected));
    }
    bus.write16(MIX, 0x1070).unwrap();
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 120,
            right: 0
        }
    );
    bus.write16(MIX, 0x0100).unwrap(); // Volume field zero means one, not mute.
    assert_eq!(bus.audio_level(), StereoLevel { left: 0, right: 15 });
    bus.write16(MIX, 0x1177).unwrap();
    bus.write16(SOUNDCNT_H, 0x0306).unwrap();
    bus.write32(FIFO_A, 0x7f7f7f7f).unwrap();
    bus.write32(TIMER_BASE, 0x0080ffff).unwrap();
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level(), stereo(511)); // 508 + 120 clips after mixing.
    bus.write8(DUTY + 1, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(508)); // DAC off removes only pulse contribution.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn length_clocks_at_256_hz_and_status_expires_independently_of_routing() {
    let mut bus = bus();
    bus.write16(MIX, 0).unwrap();
    bus.write16(DUTY, 0xf0be).unwrap(); // Two length ticks.
    bus.write16(FREQUENCY, 0xc7ff).unwrap();
    bus.advance_cycles(SEQUENCE - 1);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    bus.advance_cycles(1); // Step0, first length tick.
    bus.advance_cycles(SEQUENCE * 2 - 1);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    bus.advance_cycles(1); // Step2, second length tick.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.read16(FREQUENCY).unwrap(), 0x4000);
}

#[test]
fn envelope_64_hz_and_sweep_128_hz_use_distinct_sequencer_steps() {
    let mut bus = bus();
    bus.write16(DUTY, 0x3980).unwrap(); // Volume3, increase every envelope tick.
    bus.write16(FREQUENCY, 0x87ff).unwrap();
    bus.advance_cycles(SEQUENCE * 8 - 1);
    assert_eq!(bus.audio_level().left.abs(), 24);
    bus.advance_cycles(1); // Step7.
    assert_eq!(bus.audio_level().left.abs(), 32);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);

    let mut bus = self::bus();
    bus.write16(SWEEP, 0x11).unwrap();
    bus.write16(DUTY, 0xf080).unwrap();
    bus.write16(FREQUENCY, 0x83e8).unwrap(); // 1000 ->1500; second check overflows2250.
    bus.advance_cycles(SEQUENCE * 3 - 1);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    bus.advance_cycles(1); // Step2, first sweep clock.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
}

#[test]
fn sequencer_divider_runs_while_master_off_and_enable_resets_only_step() {
    let mut bus = Memory::new(vec![]).unwrap();
    bus.advance_cycles(SEQUENCE - 1);
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(DUTY, 0xf03f).unwrap();
    bus.write16(FREQUENCY, 0xc000).unwrap();
    bus.advance_cycles(1); // Not one whole new interval after enable.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    assert_eq!(bus.read16(DUTY).unwrap(), 0);
    bus.write16(DUTY, 0xffff).unwrap();
    bus.write16(FREQUENCY, 0xffff).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.read16(DUTY).unwrap(), 0);
}

#[test]
fn reserved_psg_ratio_errors_are_atomic_including_enable_after_disabled_configuration() {
    let mut bus = bus();
    start(&mut bus);
    let before = bus.audio_level();
    let error = bus.write32(MIX, 0x00030000).unwrap_err();
    assert!(error.to_string().contains("reserved PSG volume selection"));
    assert_eq!(bus.read16(MIX).unwrap(), 0x1177);
    assert_eq!(bus.read16(SOUNDCNT_H).unwrap(), 2);
    assert_eq!(bus.audio_level(), before);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    bus.write16(SOUNDCNT_H, 3).unwrap(); // Disabled sound still retains these bits.
    assert!(bus.write16(SOUNDCNT_X, 0x80).is_err());
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0);
}

#[test]
fn halt_keeps_pulse_running_and_stop_freezes_it() {
    for stop in [false, true] {
        let mut bus = bus();
        start(&mut bus);
        bus.write16(FREQUENCY, 0x87fd).unwrap(); // 48 clocks per duty step.
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        let cpu = machine.cpu().clone();
        machine.step().unwrap();
        assert_eq!(machine.cpu(), &cpu);
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
fn clock_batching_and_row_capture_do_not_change_psg_result() {
    let mut bulk = bus();
    let mut rows = bus();
    let mut split = bus();
    rows.set_scanline_rendering(true);
    for bus in [&mut bulk, &mut rows, &mut split] {
        bus.write16(SWEEP, 0x29).unwrap();
        bus.write16(DUTY, 0xa340).unwrap();
        bus.write16(FREQUENCY, 0x85ad).unwrap();
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
    // Maximum batch must remain bounded and preserve an indefinite channel.
    let mut bus = bus();
    start(&mut bus);
    bus.advance_cycles(u32::MAX);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x81);
    assert_eq!(bus.audio_level(), stereo(120)); // floor(MAX/16) modulo8 = 7.
}
