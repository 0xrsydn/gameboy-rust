use super::Wave;

fn start(bank: u8, frequency: u16) -> Wave {
    let mut wave = Wave::default();
    wave.write(0, 0x80 | (bank << 6), 0);
    wave.write(3, 0x20, 0);
    wave.write(4, frequency as u8, 0);
    wave.write(5, 0x80 | (frequency >> 8) as u8, 0);
    wave
}

#[test]
fn every_frequency_and_bank_follow_high_nibble_first_rotation() {
    for frequency in 0..2048 {
        for bank in 0..2 {
            let mut wave = start(bank, frequency);
            let mut ram = [[0xab; 16]; 2];
            let mut digits: Vec<u8> = (0..32).map(|i| (i * 7 % 16) as u8).collect();
            for (index, byte) in ram[bank as usize].iter_mut().enumerate() {
                *byte = digits[index * 2] * 16 + digits[index * 2 + 1];
            }
            let period = (2048 - u32::from(frequency)) * 8;
            assert_eq!(wave.sample_quarters(), 0);
            for _ in 0..65 {
                let before = ram;
                wave.advance(period - 1, &mut ram);
                assert_eq!(ram, before);
                let output = digits.remove(0);
                digits.push(output);
                wave.advance(1, &mut ram);
                assert_eq!(wave.sample_quarters(), (i16::from(output) - 8) * 8);
                for (index, byte) in ram[bank as usize].iter().enumerate() {
                    assert_eq!(*byte, digits[index * 2] * 16 + digits[index * 2 + 1]);
                }
                assert_eq!(ram[1 - bank as usize], [0xab; 16]);
            }
        }
    }
}

#[test]
fn batch_advance_matches_single_clocks_and_full_rotations_keep_the_last_sample() {
    for frequency in [0, 1024, 2047] {
        let mut fast = start(0, frequency);
        let mut slow = fast;
        let mut a = [[0x12; 16], [0xab; 16]];
        let mut b = a;
        fast.advance(100003, &mut a);
        for _ in 0..100003 {
            slow.advance(1, &mut b);
        }
        assert_eq!((fast, a), (slow, b));
    }
    let mut wave = start(0, 2047);
    let mut ram = [[0x12; 16]; 2];
    wave.advance(256, &mut ram);
    assert_eq!(wave.sample, Some(2));
    assert_eq!(ram, [[0x12; 16]; 2]);
    wave.advance(u32::MAX, &mut ram);
    assert_eq!(wave.sample, Some(1)); // floor(MAX/8) is odd.
    assert_eq!(wave.remaining, 1);
    assert_eq!(ram[0], [0x21; 16]);
}

#[test]
fn volume_mute_and_force_gain_preserve_fractional_units_and_activity() {
    let mut wave = start(0, 2047);
    for digit in 0..16 {
        let mut ram = [[digit * 17; 16]; 2];
        wave.advance(8, &mut ram);
        for control in 0..8 {
            wave.write(3, control << 5, 0);
            let gain = if control >= 4 {
                3
            } else {
                [0, 4, 2, 1][control as usize]
            };
            assert_eq!(wave.sample_quarters(), (i16::from(digit) - 8) * 2 * gain);
            assert!(wave.active);
        }
    }
}

#[test]
fn trigger_frequency_length_and_gate_have_independent_effects() {
    let mut wave = start(0, 2047);
    let mut ram = [[0x12; 16]; 2];
    wave.advance(3, &mut ram);
    wave.write(4, 0, 0); // Change next reload, not the remaining five cycles.
    assert_eq!(wave.remaining, 5);
    wave.advance(5, &mut ram);
    assert_eq!(wave.sample, Some(1));
    assert_eq!(wave.remaining, 2048);
    wave.write(5, 0x87, 0); // Retrigger: fresh period, rotated RAM is not restored.
    assert_eq!(wave.sample, None);
    wave.advance(2048, &mut ram);
    assert_eq!(wave.sample, Some(2));
    wave.write(0, 0, 0);
    assert!(!wave.active);
    let before = ram;
    wave.advance(1000000, &mut ram);
    assert_eq!(ram, before);
    wave.write(0, 0x80, 0);
    assert!(!wave.active); // Gate enable alone is not a trigger.
    wave.write(2, 255, 0);
    wave.write(5, 0xc0, 0);
    assert_eq!(wave.length, 1);
    wave.clock(1);
    assert!(wave.active);
    wave.clock(2);
    assert!(!wave.active);
    wave.write(5, 0xc0, 1);
    assert_eq!(wave.length, 255);
    assert!(wave.active);
}

#[test]
fn length_enable_extra_clock_and_muted_trigger_do_not_reset_ram() {
    let mut wave = start(0, 2047);
    wave.write(2, 255, 0);
    wave.write(5, 0x47, 1);
    assert_eq!(wave.length, 0);
    assert!(!wave.active);
    wave.write(0, 0, 0);
    wave.write(5, 0xc7, 1);
    assert_eq!(wave.length, 255);
    assert!(!wave.active);
    for _ in 0..255 {
        wave.clock(0);
    }
    assert_eq!(wave.length, 0);
}
