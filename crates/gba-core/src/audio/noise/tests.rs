//! Original counter references and deterministic clock-batching checks.
use super::*;

fn start(polynomial: u8) -> Noise {
    let mut noise = Noise::default();
    noise.write(1, 0xf0, 0);
    noise.write(4, polynomial, 0);
    noise.write(5, 0x80, 0);
    noise
}

// Independent bit-array form of the documented Galois counter.
fn reference(state: u16, short: bool) -> (u16, bool) {
    let mut bits = [false; 15];
    for (index, bit) in bits.iter_mut().enumerate() {
        *bit = state & (1 << index) != 0;
    }
    let carry = bits[0];
    bits.copy_within(1..15, 0);
    bits[14] = false;
    if carry {
        let top = if short { 6 } else { 14 };
        bits[top] = !bits[top];
        bits[top - 1] = !bits[top - 1];
    }
    let value = bits
        .iter()
        .enumerate()
        .fold(0, |value, (index, &bit)| value | (u16::from(bit) << index));
    (value, carry)
}

#[test]
fn both_counters_visit_every_nonzero_state_before_repeating() {
    for (short, width) in [(false, 15), (true, 7)] {
        let initial = 1 << (width - 1);
        let mut state = initial;
        let mut seen = vec![false; 1 << width];
        let mut positive = 0;
        for index in 0..(1 << width) - 1 {
            assert_ne!(state, 0);
            assert!(!seen[usize::from(state)]);
            seen[usize::from(state)] = true;
            let (expected, high) = reference(state, short);
            assert_eq!(shift(state, short), expected);
            if index < width {
                assert_eq!(high, index == width - 1);
            }
            positive += usize::from(high);
            state = expected;
        }
        assert_eq!(state, initial);
        assert_eq!(positive, 1 << (width - 1));
    }
}

#[test]
fn jump_maps_match_each_state_and_iterated_random_batches_including_transient_upper_bits() {
    for short in [false, true] {
        for state in 0..=0x7fff {
            assert_eq!(jump(state, 1, short), reference(state, short).0);
        }
        let mut random = 0x4183u32;
        for _ in 0..200 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let state = (random & 0x7fff) as u16;
            let count = (random >> 16) & 4095;
            let mut expected = state;
            for _ in 0..count {
                expected = reference(expected, short).0;
            }
            assert_eq!(jump(state, count, short), expected);
            assert_eq!(
                jump(jump(state, count / 2, short), count - count / 2, short),
                expected
            );
        }
    }
}

#[test]
fn all_divider_shift_width_fields_observe_exact_nominal_edges() {
    for polynomial in 0..=255u8 {
        let divisor = [32, 64, 128, 192, 256, 320, 384, 448][usize::from(polynomial & 7)];
        let period = divisor * (1u32 << (polynomial >> 4));
        for cycles in [0, period - 1, period, period * 19 + 7] {
            let mut noise = start(polynomial);
            noise.advance(cycles);
            let mut expected = if polynomial & 8 != 0 { 0x40 } else { 0x4000 };
            let mut high = false;
            for _ in 0..cycles / period {
                (expected, high) = reference(expected, polynomial & 8 != 0);
            }
            assert_eq!(noise.lfsr, expected);
            assert_eq!(noise.high, high);
            assert_eq!(noise.remaining, period - cycles % period);
            assert_eq!(noise.sample(), if high { 15 } else { -15 });
        }
    }
}

#[test]
fn maximum_clock_batch_matches_independent_cycle_reduction_and_last_carry() {
    for (polynomial, width) in [(0, 15), (8, 7)] {
        let mut noise = start(polynomial);
        noise.advance(u32::MAX);
        let edges = u32::MAX / 32;
        let mut expected = 1 << (width - 1);
        for _ in 0..(edges - 1) % ((1 << width) - 1) {
            expected = reference(expected, width == 7).0;
        }
        let (state, high) = reference(expected, width == 7);
        assert_eq!((noise.lfsr, noise.high), (state, high));
        assert_eq!(noise.remaining, 1);
    }
}

#[test]
fn live_width_and_rate_changes_preserve_counter_and_remaining_interval() {
    let mut noise = start(0);
    noise.advance(3 * 32 + 15);
    assert_eq!(noise.lfsr, 0x800);
    noise.write(4, 0x19, 0); // Short mode, new period128. Old interval has17 clocks left.
    let mut expected = noise.lfsr;
    noise.advance(16);
    assert_eq!(noise.lfsr, expected);
    noise.advance(1);
    expected = reference(expected, true).0;
    assert_eq!(noise.lfsr, expected); // Upper bits are not truncated or reseeded.
    assert_eq!(noise.remaining, 128);
    noise.advance(128 * 1000);
    for _ in 0..1000 {
        expected = reference(expected, true).0;
    }
    assert_eq!(noise.lfsr, expected);
    noise.write(4, 0, 0);
    noise.advance(128 + 32 * 25);
    for _ in 0..26 {
        expected = reference(expected, false).0;
    }
    assert_eq!(noise.lfsr, expected);
}

#[test]
fn trigger_reseeds_counter_reloads_timer_and_resets_held_output() {
    for (polynomial, width) in [(0, 15), (8, 7)] {
        let mut noise = start(polynomial);
        noise.advance(width * 32);
        assert_eq!(noise.sample(), 15);
        noise.write(5, 0x80, 0);
        assert_eq!(noise.lfsr, 1 << (width - 1));
        assert_eq!(noise.remaining, 32);
        assert_eq!(noise.sample(), -15);
        noise.write(1, 0, 0);
        let before = noise;
        noise.advance(u32::MAX);
        assert_eq!(noise, before);
        assert_eq!(noise.sample(), 0);
    }
}

#[test]
fn zero_cycles_and_arbitrary_splits_preserve_full_noise_state() {
    let mut bulk = start(8);
    let mut split = bulk;
    for cycles in [0, 1, 31, 33, 4096, 1000000, u32::MAX] {
        bulk.advance(cycles);
        split.advance(cycles / 2);
        split.advance(cycles - cycles / 2);
        assert_eq!(bulk, split);
    }
}
