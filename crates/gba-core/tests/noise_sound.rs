//! Original public-bus regressions for noise channel 4. No game audio or assets.
use gba_core::{
    audio::StereoLevel,
    cpu::Cpu,
    io::{FIFO_A, HALTCNT, IF, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, TIMER_BASE},
    machine::Machine,
    memory::{Memory, ROM_START},
};

const ENVELOPE: u32 = 0x04000078;
const CONTROL: u32 = 0x0400007c;
const MIX: u32 = 0x04000080;
const SEQUENCE: u32 = 32768;

fn bus() -> Memory {
    let mut bus = Memory::new(0xeafffffeu32.to_le_bytes().to_vec()).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(MIX, 0x8877).unwrap();
    bus.write16(SOUNDCNT_H, 2).unwrap();
    bus
}

fn start(bus: &mut Memory, short: bool) {
    bus.write16(ENVELOPE, 0xf000).unwrap();
    bus.write16(CONTROL, if short { 0x8008 } else { 0x8000 })
        .unwrap();
}

fn stereo(value: i16) -> StereoLevel {
    StereoLevel {
        left: value,
        right: value,
    }
}

fn reference_level(cycles: u32, short: bool) -> i16 {
    let edges = cycles / 32;
    if edges == 0 {
        return -120;
    }
    let (mut state, mask, period) = if short {
        (0x40u16, 0x60, 127)
    } else {
        (0x4000, 0x6000, 32767)
    };
    for _ in 0..(edges - 1) % period {
        state = (state >> 1) ^ if state & 1 != 0 { mask } else { 0 };
    }
    if state & 1 != 0 {
        120
    } else {
        -120
    }
}

#[test]
fn register_masks_padding_readonly_status_and_first_carry_in_both_widths() {
    for (short, width) in [(false, 15), (true, 7)] {
        let mut bus = bus();
        bus.write32(ENVELOPE, 0xfffff0ff).unwrap(); // Unused bits/halfword cannot affect control.
        assert_eq!(bus.read32(ENVELOPE).unwrap(), 0xf000);
        assert_eq!(bus.read16(CONTROL).unwrap(), 0);
        bus.write8(CONTROL, if short { 8 } else { 0 }).unwrap();
        bus.write8(CONTROL + 1, 0xff).unwrap();
        assert_eq!(
            bus.read32(CONTROL).unwrap(),
            if short { 0x4008 } else { 0x4000 }
        );
        assert_eq!(bus.read32(SOUNDCNT_X).unwrap(), 0x88);
        bus.write16(SOUNDCNT_X, 0x8f).unwrap();
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
        assert_eq!(bus.audio_level(), stereo(-120));
        bus.advance_cycles(width * 32 - 1);
        assert_eq!(bus.audio_level(), stereo(-120));
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(120));
        assert_eq!(bus.read16(IF).unwrap(), 0);
    }
}

#[test]
fn length_expiry_and_extra_clock_reload_do_not_need_a_general_timer() {
    let mut bus = bus();
    bus.write16(ENVELOPE, 0xf03e).unwrap();
    bus.write16(CONTROL, 0xc000).unwrap();
    bus.advance_cycles(SEQUENCE);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
    bus.advance_cycles(SEQUENCE * 2);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.write16(CONTROL, 0xc000).unwrap(); // Next step3: empty length reloads63.
    bus.advance_cycles(SEQUENCE * 125);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
    bus.advance_cycles(SEQUENCE);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn envelope_changes_at_shared_step7_and_saturation_does_not_clear_status() {
    let mut bus = bus();
    bus.advance_cycles(SEQUENCE * 6);
    bus.write16(ENVELOPE, 0x3900).unwrap();
    bus.write16(CONTROL, 0x8008).unwrap();
    bus.advance_cycles(SEQUENCE * 2 - 1);
    assert_eq!(bus.audio_level().left.abs(), 24);
    bus.advance_cycles(1);
    assert_eq!(bus.audio_level().left.abs(), 32);
    bus.write16(ENVELOPE, 0xc100).unwrap();
    bus.write16(CONTROL, 0x8008).unwrap();
    bus.advance_cycles(SEQUENCE * 8 * 20);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
    assert_eq!(bus.audio_level(), stereo(0));
}

#[test]
fn master_off_and_dac_gate_prevent_activation_until_reconfigured_and_triggered() {
    let mut bus = bus();
    start(&mut bus, false);
    bus.write8(ENVELOPE + 1, 0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.write8(ENVELOPE + 1, 0xf0).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(CONTROL, 0x8000).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    bus.write32(ENVELOPE, u32::MAX).unwrap();
    bus.write32(CONTROL, u32::MAX).unwrap();
    assert_eq!(bus.read32(ENVELOPE).unwrap(), 0);
    assert_eq!(bus.read32(CONTROL).unwrap(), 0);
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(CONTROL, 0x8000).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn independent_routing_gain_ratios_and_sum_before_rounding_include_noise() {
    let mut bus = bus();
    start(&mut bus, false);
    for (ratio, expected) in [(0, -30), (1, -60), (2, -120)] {
        bus.write16(SOUNDCNT_H, ratio).unwrap();
        assert_eq!(bus.audio_level(), stereo(expected));
    }
    bus.write16(MIX, 0x8070).unwrap();
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: -120,
            right: 0
        }
    );
    bus.write16(MIX, 0x0800).unwrap();
    assert_eq!(
        bus.audio_level(),
        StereoLevel {
            left: 0,
            right: -15
        }
    );
    bus.write16(MIX, 0x9900).unwrap();
    bus.write16(SOUNDCNT_H, 0).unwrap();
    bus.write16(0x04000062, 0x2080).unwrap(); // Pulse1 +2.
    bus.write16(0x04000064, 0x8000).unwrap();
    bus.write16(ENVELOPE, 0x1000).unwrap(); // Noise -1.
    bus.write16(CONTROL, 0x8000).unwrap();
    assert_eq!(bus.audio_level(), stereo(0)); // Sum1 >>2, not separately rounded0 + -1.
}

#[test]
fn all_supported_sources_mix_before_bias_clipping_and_noise_mute_does_not_stop_it() {
    for positive in [false, true] {
        let mut bus = bus();
        bus.write16(MIX, 0x9977).unwrap(); // Pulse1 and noise, both sides.
        bus.write16(SOUNDCNT_H, 0x0306).unwrap(); // Also Direct Sound A.
        bus.write16(0x04000062, if positive { 0xf080 } else { 0xf000 })
            .unwrap();
        bus.write16(0x04000064, 0x8000).unwrap();
        start(&mut bus, true);
        if positive {
            bus.advance_cycles(7 * 32);
        }
        bus.write32(FIFO_A, if positive { 0x7f7f7f7f } else { 0x80808080 })
            .unwrap();
        bus.write32(TIMER_BASE, 0x0080ffff).unwrap();
        bus.advance_cycles(1);
        assert_eq!(bus.audio_level(), stereo(if positive { 511 } else { -512 }));
        bus.write16(MIX, 0).unwrap();
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x89);
        assert_eq!(bus.audio_level(), stereo(if positive { 508 } else { -512 }));
    }
}

#[test]
fn noise_status_and_length_are_independent_of_both_pulses() {
    let mut bus = bus();
    for (duty, frequency) in [(0x04000062, 0x04000064), (0x04000068, 0x0400006c)] {
        bus.write16(duty, 0xf080).unwrap();
        bus.write16(frequency, 0x87ff).unwrap();
    }
    bus.write16(ENVELOPE, 0xf03f).unwrap();
    bus.write16(CONTROL, 0xc000).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x8b);
    bus.advance_cycles(SEQUENCE);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x83);
    assert_eq!(bus.audio_level(), stereo(0)); // Pulses are not routed here.
}

#[test]
fn halt_keeps_noise_and_modulation_running_but_stop_freezes_them() {
    for stop in [false, true] {
        let mut bus = bus();
        start(&mut bus, true);
        bus.write8(HALTCNT, if stop { 0x80 } else { 0 }).unwrap();
        let mut machine = Machine::new(Cpu::new(ROM_START), bus);
        let before = machine.cpu().clone();
        machine.step().unwrap();
        assert_eq!(machine.cpu(), &before);
        assert_eq!(
            machine.memory().audio_level(),
            stereo(reference_level(machine.cycles() as u32, true))
        );
        if stop {
            machine.memory_mut().advance_cycles(u32::MAX);
            assert_eq!(machine.cycles(), 0);
            assert_eq!(machine.memory().audio_level(), stereo(-120));
        } else {
            assert!(machine.cycles() > 0);
        }
    }
}

#[test]
fn batched_single_clock_and_row_capture_paths_agree_and_maximum_batches_are_bounded() {
    let mut bulk = bus();
    let mut rows = bus();
    let mut small = bus();
    rows.set_scanline_rendering(true);
    for bus in [&mut bulk, &mut rows, &mut small] {
        bus.write16(ENVELOPE, 0xc100).unwrap();
        bus.write16(CONTROL, 0x8008).unwrap();
    }
    for cycles in [1, 31, 4096, 32768, 262145] {
        bulk.advance_cycles(cycles);
        rows.advance_cycles(cycles);
        for _ in 0..cycles {
            small.advance_cycles(1);
        }
        assert_eq!(bulk.audio_level(), small.audio_level());
        assert_eq!(bulk.audio_level(), rows.audio_level());
        assert_eq!(
            bulk.read16(SOUNDCNT_X).unwrap(),
            rows.read16(SOUNDCNT_X).unwrap()
        );
    }
    for short in [false, true] {
        let mut bus = bus();
        start(&mut bus, short);
        bus.advance_cycles(u32::MAX);
        assert_eq!(bus.audio_level(), stereo(reference_level(u32::MAX, short)));
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x88);
    }
}
