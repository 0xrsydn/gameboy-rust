//! Original channel 3 tests. Digital levels are not host audio or hardware recordings.
use gba_core::{
    audio::StereoLevel,
    io::{HALTCNT, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_X, WAVE_RAM},
    memory::Memory,
};
const CONTROL: u32 = 0x04000070;
const LENGTH: u32 = 0x04000072;
const FREQUENCY: u32 = 0x04000074;
const ROUTE: u32 = 0x04000080;

fn bus() -> Memory {
    let mut bus = Memory::new(vec![]).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    bus.write16(SOUNDCNT_H, 2).unwrap();
    bus.write16(ROUTE, 0x4477).unwrap();
    bus
}
fn start(bus: &mut Memory, bank: u16, data: u8) {
    bus.write16(CONTROL, (bank ^ 1) << 6).unwrap();
    for offset in 0..16 {
        bus.write8(WAVE_RAM + offset, data).unwrap();
    }
    bus.write16(CONTROL, 0x80 | (bank << 6)).unwrap();
    bus.write16(LENGTH, 0x2000).unwrap();
    bus.write16(FREQUENCY, 0x87ff).unwrap();
}
fn stereo(level: i16) -> StereoLevel {
    StereoLevel {
        left: level,
        right: level,
    }
}

#[test]
fn register_masks_gate_status_and_unused_halfword_do_not_alias_other_channels() {
    let mut bus = bus();
    bus.write16(CONTROL, 0xffff).unwrap();
    bus.write16(LENGTH, 0xffff).unwrap();
    bus.write16(FREQUENCY, 0x7fff).unwrap(); // No trigger, 64-sample configuration is idle.
    bus.write16(FREQUENCY + 2, 0xffff).unwrap();
    assert_eq!(bus.read16(CONTROL).unwrap(), 0xe0);
    assert_eq!(bus.read16(LENGTH).unwrap(), 0xe000);
    assert_eq!(bus.read16(FREQUENCY).unwrap(), 0x4000);
    assert_eq!(bus.read16(FREQUENCY + 2).unwrap(), 0);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    bus.write16(CONTROL, 0).unwrap();
    bus.write16(FREQUENCY, 0x87ff).unwrap(); // A trigger with the gate off cannot play.
    bus.advance_cycles(1000000);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
    start(&mut bus, 0, 0xf0);
    bus.write16(SOUNDCNT_X, 0xff).unwrap(); // Status is read-only.
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x84);
    assert_eq!(bus.audio_level(), stereo(0));
    bus.advance_cycles(8);
    assert_eq!(bus.audio_level(), stereo(112));
    bus.advance_cycles(8);
    assert_eq!(bus.audio_level(), stereo(-128));
    bus.write16(CONTROL, 0).unwrap();
    bus.write16(CONTROL, 0x80).unwrap();
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn cpu_bank_access_and_readback_follow_rotated_storage_without_touching_the_other_bank() {
    for bank in [0, 1] {
        let mut bus = bus();
        start(&mut bus, bank, 0x12);
        bus.write32(WAVE_RAM, 0xabcdef90).unwrap(); // Other bank remains writable during playback.
        bus.advance_cycles(8);
        assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0xabcdef90);
        bus.write16(CONTROL, (bank ^ 1) << 6).unwrap(); // Stop and expose the played bank.
        assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0x21212121);
        bus.write16(SOUNDCNT_X, 0).unwrap();
        bus.write16(SOUNDCNT_X, 0x80).unwrap();
        bus.write16(CONTROL, (bank ^ 1) << 6).unwrap();
        assert_eq!(bus.read32(WAVE_RAM).unwrap(), 0x21212121);
        assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    }
}

#[test]
fn wave_gain_routes_and_psg_ratio_use_one_final_signed_rounding() {
    let mut bus = bus();
    start(&mut bus, 1, 0x99); // Centered amplitude +2 before volume.
    bus.advance_cycles(8);
    for ratio in 0..3 {
        bus.write16(SOUNDCNT_H, ratio).unwrap();
        for setting in 0..8 {
            bus.write8(LENGTH + 1, setting << 5).unwrap();
            let gain = if setting >= 4 {
                3
            } else {
                [0, 4, 2, 1][setting as usize]
            };
            for volume in 0..8 {
                bus.write16(ROUTE, 0x4400 | volume | (volume << 4)).unwrap();
                let expected = (2 * gain * (volume as i16 + 1) * (1 << ratio)) >> 4;
                assert_eq!(bus.audio_level(), stereo(expected));
            }
        }
    }
    bus.write16(ROUTE, 0x0400).unwrap();
    assert_eq!(bus.audio_level(), StereoLevel { left: 0, right: 1 }); // +1.5 rounds once.
    bus.write16(ROUTE, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(0));
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x84);
}

#[test]
fn wave_fraction_combines_with_pulse_before_rounding_and_direct_sound_bias_clipping() {
    let mut bus = bus();
    start(&mut bus, 0, 0x77); // -2 amplitude.
    bus.advance_cycles(8);
    bus.write16(LENGTH, 0x8000).unwrap(); // -1.5 at forced 75%.
    bus.write16(0x04000062, 0x1080).unwrap(); // Pulse amplitude +1 at duty position zero.
    bus.write16(0x04000064, 0x8000).unwrap();
    bus.write16(ROUTE, 0x5500).unwrap();
    assert_eq!(bus.audio_level(), stereo(-1)); // (-1.5 + 1), not individually rounded by a different rule.
    bus.write16(SOUNDBIAS, 0).unwrap();
    assert_eq!(bus.audio_level(), stereo(-512));
    bus.write16(SOUNDBIAS, 0x3fe).unwrap();
    bus.write16(0x04000062, 0xf080).unwrap();
    bus.write16(0x04000064, 0x8000).unwrap();
    bus.write16(ROUTE, 0x5577).unwrap();
    assert_eq!(bus.audio_level(), stereo(511));
    // Fractional wave and pulse contributions must not be rounded separately.
    bus.write16(SOUNDBIAS, 0x200).unwrap();
    start(&mut bus, 0, 0x99);
    bus.advance_cycles(8);
    bus.write16(LENGTH, 0x8000).unwrap(); // +1.5.
    bus.write16(0x04000062, 0x1080).unwrap();
    bus.write16(0x04000064, 0x8000).unwrap(); // +1.
    bus.write16(ROUTE, 0x5500).unwrap();
    bus.write16(SOUNDCNT_H, 1).unwrap(); // 50%: (+1.5 + 1)/2, rounded once.
    assert_eq!(bus.audio_level(), stereo(1));
}

#[test]
fn length_shared_clock_halt_stop_and_master_disable_have_distinct_effects() {
    let mut bus = bus();
    start(&mut bus, 0, 0xf0);
    bus.write16(LENGTH, 0x20ff).unwrap(); // One length tick.
    bus.write16(FREQUENCY, 0xc7ff).unwrap();
    bus.write8(HALTCNT, 0).unwrap();
    bus.advance_cycles(32767);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x84);
    bus.advance_cycles(1);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
    assert_eq!(bus.audio_level(), stereo(0));
    start(&mut bus, 0, 0xf0);
    bus.advance_cycles(8);
    bus.write8(HALTCNT, 0x80).unwrap();
    let before = bus.audio_level();
    bus.advance_cycles(1000000);
    assert_eq!(bus.audio_level(), before);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x84);
    bus.write16(SOUNDCNT_X, 0).unwrap();
    bus.write32(CONTROL, u32::MAX).unwrap(); // Disabled writes ignored.
    bus.write16(FREQUENCY, 0xffff).unwrap();
    bus.write16(SOUNDCNT_X, 0x80).unwrap();
    assert_eq!(bus.read32(CONTROL).unwrap(), 0);
    assert_eq!(bus.read16(SOUNDCNT_X).unwrap(), 0x80);
}

#[test]
fn unsupported_live_bank_changes_and_two_bank_playback_are_retryable_and_atomic() {
    let mut bus = bus();
    start(&mut bus, 0, 0x12);
    bus.advance_cycles(8);
    let before = bus.audio_level();
    for value in [0xc0, 0xa0, 0xe0] {
        let error = bus.write32(CONTROL, 0xe0000000 | value).unwrap_err();
        assert_eq!(bus.write32(CONTROL, 0xe0000000 | value), Err(error));
        assert_eq!(bus.read16(CONTROL).unwrap(), 0x80);
        assert_eq!(bus.read16(LENGTH).unwrap(), 0x2000);
        assert_eq!(bus.audio_level(), before);
    }
    bus.write16(CONTROL, 0).unwrap();
    bus.write16(CONTROL, 0xa0).unwrap();
    let error = bus.write16(FREQUENCY, 0xffff).unwrap_err();
    assert!(error.to_string().contains("64-sample wave playback"));
    assert_eq!(bus.write16(FREQUENCY, 0xffff), Err(error));
    assert_eq!(bus.read16(FREQUENCY).unwrap(), 0);
}

#[test]
fn batched_clocks_match_small_chunks_with_and_without_scanline_capture() {
    let mut a = bus();
    let mut b = bus();
    for bus in [&mut a, &mut b] {
        start(bus, 1, 0x12);
    }
    b.set_scanline_rendering(true);
    a.advance_cycles(100003);
    for _ in 0..100003 {
        b.advance_cycles(1);
    }
    assert_eq!(a.audio_level(), b.audio_level());
    for bus in [&mut a, &mut b] {
        bus.write16(CONTROL, 0).unwrap();
    }
    for offset in 0..16 {
        assert_eq!(a.read8(WAVE_RAM + offset), b.read8(WAVE_RAM + offset));
    }
}
