//! Original arithmetic and state-transition regressions, not imported hardware tests.
use super::*;

fn start(frequency: u16, duty: u8, envelope: u8, sweep: u8) -> Pulse {
    let mut pulse = Pulse::default();
    pulse.write(0, sweep, 0);
    pulse.write(2, duty << 6, 0);
    pulse.write(3, envelope, 0);
    pulse.write(4, frequency as u8, 0);
    pulse.write(5, 0x80 | (frequency >> 8) as u8, 0);
    pulse
}

#[test]
fn all_frequencies_and_duties_match_independent_edge_arithmetic() {
    let high_phases: [&[u8]; 4] = [&[7], &[0, 7], &[0, 5, 6, 7], &[1, 2, 3, 4, 5, 6]];
    for frequency in 0..2048 {
        let period = 16 * (2048 - u32::from(frequency));
        for duty in 0..4 {
            for cycles in [0, period - 1, period, period * 8, u32::MAX] {
                let mut pulse = start(frequency, duty, 0xf0, 0);
                pulse.advance(cycles);
                let phase = ((cycles / period) % 8) as u8;
                assert_eq!(pulse.phase, phase);
                assert_eq!(pulse.remaining, period - cycles % period);
                assert_eq!(
                    pulse.sample(),
                    if high_phases[duty as usize].contains(&phase) {
                        15
                    } else {
                        -15
                    }
                );
            }
        }
    }
}

#[test]
fn frequency_writes_preserve_current_interval_and_trigger_preserves_duty_phase() {
    let mut pulse = start(2046, 2, 0xf0, 0); // 32 clocks per edge.
    pulse.advance(31);
    pulse.write(4, 255, 0); // Next period is 16 clocks; current interval has one left.
    pulse.advance(1);
    assert_eq!(pulse.phase, 1);
    assert_eq!(pulse.remaining, 16);
    pulse.advance(7);
    pulse.write(5, 0x87, 0);
    assert_eq!(pulse.phase, 1);
    assert_eq!(pulse.remaining, 16);
    pulse.advance(15);
    assert_eq!(pulse.phase, 1);
    pulse.advance(1);
    assert_eq!(pulse.phase, 2);
}

#[test]
fn length_load_enable_extra_clock_expiry_and_zero_reload() {
    let mut pulse = start(1000, 2, 0xf0, 0);
    pulse.write(2, 63, 0); // One remaining length tick.
    pulse.clock(0); // Disabled length does not expire.
    assert!(pulse.active);
    pulse.write(5, 0x43, 1); // Enable before a step that does not clock length.
    assert_eq!(pulse.length, 0);
    assert!(!pulse.active);
    pulse.write(5, 0xc3, 1); // Trigger with empty length reloads 63 in this phase.
    assert_eq!(pulse.length, 63);
    assert!(pulse.active);
    for _ in 0..62 {
        pulse.clock(0);
    }
    assert!(pulse.active);
    pulse.clock(0);
    assert!(!pulse.active);
    pulse.clock(0);
    assert_eq!(pulse.length, 0);
    pulse.write(5, 0xc3, 0);
    assert_eq!(pulse.length, 64);
}

#[test]
fn simultaneous_length_enable_and_trigger_reload_after_extra_expiry() {
    let mut pulse = start(1000, 0, 0xf0, 0);
    pulse.write(2, 63, 0);
    pulse.write(5, 0xc3, 1);
    assert_eq!(pulse.length, 63);
    assert!(pulse.active);
    pulse.write(2, 62, 0);
    pulse.write(5, 0xc3, 1); // Already enabled: do not add another extra decrement.
    assert_eq!(pulse.length, 2);
}

#[test]
fn envelopes_step_at_selected_period_saturate_and_do_not_clear_status() {
    for envelope in [0x39, 0xc1] {
        let mut pulse = start(0, 1, envelope, 0);
        for tick in 1..=20 {
            pulse.clock(7);
            let expected = if envelope & 8 != 0 {
                (3 + tick).min(15)
            } else {
                12u8.saturating_sub(tick)
            };
            assert_eq!(pulse.volume, expected);
            assert!(pulse.active);
        }
    }
    let mut pulse = start(0, 1, 0x53, 0);
    pulse.clock(7);
    pulse.clock(7);
    assert_eq!(pulse.volume, 5);
    pulse.clock(7);
    assert_eq!(pulse.volume, 4);
    pulse.write(3, 0xa0, 0); // Period zero holds current volume until retrigger.
    for _ in 0..16 {
        pulse.clock(7);
    }
    assert_eq!(pulse.volume, 4);
    pulse.write(5, 0x80, 0);
    assert_eq!(pulse.volume, 10);
}

#[test]
fn live_envelope_configuration_retains_volume_and_dac_off_requires_retrigger() {
    let mut pulse = start(0, 1, 0x52, 0);
    pulse.clock(7);
    pulse.write(3, 0xa9, 0); // Change direction/period, not current volume or remaining tick.
    assert_eq!(pulse.volume, 5);
    pulse.clock(7);
    assert_eq!(pulse.volume, 6);
    pulse.write(3, 0, 0);
    assert!(!pulse.active);
    assert_eq!(pulse.sample(), 0);
    pulse.write(3, 0x08, 0); // Rising envelope with initial zero has DAC enabled.
    assert!(!pulse.active);
    pulse.write(5, 0x80, 0);
    assert!(pulse.active);
    assert_eq!(pulse.volume, 0);
    pulse.write(3, 7, 0); // Period alone cannot enable DAC.
    pulse.write(5, 0x80, 0);
    assert!(!pulse.active);
}

#[test]
fn sweep_checks_trigger_overflow_and_second_calculation_after_update() {
    assert!(!start(1500, 1, 0xf0, 0x11).active);
    assert!(!start(1500, 1, 0xf0, 1).active); // Time zero still checks at trigger.
    let mut pulse = start(1000, 1, 0xf0, 0x11);
    assert!(pulse.active);
    pulse.clock(2);
    assert_eq!(pulse.frequency, 1500);
    assert!(!pulse.active); // Next calculated 2250 disables before another sweep tick.
}

#[test]
fn sweep_uses_shadow_frequency_and_preserves_live_write_until_next_update() {
    let mut pulse = start(400, 1, 0xf0, 0x21);
    pulse.write(4, 200, 0);
    assert_eq!(pulse.frequency, 456);
    pulse.clock(2);
    assert_eq!(pulse.frequency, 456);
    pulse.clock(6);
    assert_eq!(pulse.frequency, 600); // 400 + 400/2, not based on 456.
    assert_eq!(pulse.read(4), 0); // Frequency is write-only.
    assert_eq!(pulse.read(5), 0);
}

#[test]
fn sweep_zero_period_shift_zero_and_subtraction_direction_rules() {
    let mut pulse = start(500, 1, 0xf0, 1);
    for _ in 0..32 {
        pulse.clock(2);
    }
    assert_eq!(pulse.frequency, 500); // Time zero suppresses periodic changes.
    let mut pulse = start(1200, 1, 0xf0, 0x10);
    assert!(pulse.active); // Shift zero skips trigger-time overflow check.
    pulse.clock(2);
    assert!(!pulse.active); // Timed overflow check still applies.
    let mut pulse = start(1000, 1, 0xf0, 0x19);
    pulse.clock(2);
    assert_eq!(pulse.frequency, 500);
    pulse.write(0, 0x11, 0);
    assert!(!pulse.active); // Clearing negate after subtraction disables the channel.
    let mut pulse = start(1000, 1, 0xf0, 0x18);
    pulse.write(0, 0x10, 0); // No subtraction calculation has happened.
    assert!(pulse.active);
}

#[test]
fn split_clock_advances_match_one_batch() {
    let mut random = 0x5a17u32;
    let mut bulk = start(1783, 3, 0xa2, 0x29);
    let mut split = bulk;
    for step in 0..1000u32 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let cycles = random & 0xffff;
        bulk.advance(cycles);
        for part in [cycles / 3, cycles / 3, cycles - (cycles / 3) * 2] {
            split.advance(part);
        }
        bulk.clock((step & 7) as u8);
        split.clock((step & 7) as u8);
        assert_eq!(bulk, split);
    }
}
