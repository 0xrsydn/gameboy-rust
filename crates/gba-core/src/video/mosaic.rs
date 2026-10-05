//! Vertical mosaic phase for row capture. Size writes preserve four-bit counters.
//! Background phase follows the displayed row. OBJ phase runs one row ahead
//! for sprite preparation. Per-access size latching is not modeled.

use crate::display::{CYCLES_PER_FRAME, CYCLES_PER_LINE, VISIBLE_LINES};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerticalMosaic {
    pub background: u8,
    pub object: u8,
    started: bool,
}

impl VerticalMosaic {
    pub fn advance(&mut self, phase: u32, cycles: u32, sizes: u16) {
        if cycles == 0 {
            return;
        }
        let object_height = u32::from(sizes >> 12) + 1;
        if !self.started {
            // Reset has no preceding row227. Row0 uses phase0; row1 uses the next phase.
            self.object = u8::from(object_height != 1);
            self.started = true;
        }
        self.background = progress(
            phase,
            cycles,
            self.background,
            u32::from((sizes >> 4) & 15) + 1,
        )
        .counter;
        self.object = progress(
            (phase + CYCLES_PER_LINE) % CYCLES_PER_FRAME,
            cycles,
            self.object,
            object_height,
        )
        .counter;
    }
}

pub(super) struct Progress {
    pub counter: u8,
    /// Number of zero transitions since the last frame reload, if any.
    pub resets: u32,
    pub first_reset: bool,
    pub frame_reloaded: bool,
    pub crossed_line: bool,
}

/// Summarize fixed-size line transitions without iterating over lines or frames.
/// A frame wrap discards earlier history because affine origins reload at row0.
pub(super) fn progress(phase: u32, cycles: u32, counter: u8, height: u32) -> Progress {
    debug_assert!(counter < 16 && (1..=16).contains(&height));
    let end = u64::from(phase) + u64::from(cycles);
    let wrapped = end >= u64::from(CYCLES_PER_FRAME);
    let first = if wrapped { 0 } else { phase / CYCLES_PER_LINE };
    let last = (end % u64::from(CYCLES_PER_FRAME)) as u32 / CYCLES_PER_LINE;
    let initial = if wrapped { 0 } else { u32::from(counter) };
    let first_reset = first < VISIBLE_LINES
        && (first == VISIBLE_LINES - 1 || initial + 1 == height || initial == 15);
    // Normal increments enter rows1..159. Entry to row160 unconditionally resets.
    let lines = last.min(VISIBLE_LINES - 1) - first.min(VISIBLE_LINES - 1);
    let until_zero = if initial < height {
        height - initial
    } else {
        16 - initial
    };
    let (mut current, mut resets) = if lines < until_zero {
        (initial + lines, 0)
    } else {
        let remaining = lines - until_zero;
        (remaining % height, 1 + remaining / height)
    };
    if first < VISIBLE_LINES && last >= VISIBLE_LINES {
        current = 0;
        resets += 1;
    }
    Progress {
        counter: current as u8,
        resets,
        first_reset,
        frame_reloaded: wrapped,
        crossed_line: wrapped || first != last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_counter_height_and_visible_line_matches_iterated_transitions() {
        for counter in 0..16 {
            for height in 1..=16 {
                for first in 0..160 {
                    let mut expected = counter;
                    let mut resets = 0;
                    for last in first + 1..=160 {
                        let next = expected + 1;
                        expected = if last == 160 || next == height {
                            0
                        } else {
                            next & 15
                        };
                        resets += u32::from(expected == 0);
                        let p = progress(
                            first * CYCLES_PER_LINE,
                            (last - first) * CYCLES_PER_LINE,
                            counter as u8,
                            height,
                        );
                        assert_eq!(
                            u32::from(p.counter),
                            expected,
                            "counter={counter} height={height} first={first} last={last}"
                        );
                        assert_eq!(p.resets, resets);
                        assert_eq!(
                            p.first_reset,
                            first == 159 || counter + 1 == height || counter == 15
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn random_clock_batches_and_maximum_batch_match_line_iteration() {
        let mut seed = 0x3712_bc09u32;
        for case in 0..500 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let phase = next() % CYCLES_PER_FRAME;
            let cycles = if case == 0 {
                u32::MAX
            } else {
                next() % (7 * CYCLES_PER_FRAME)
            };
            let height = next() % 16 + 1;
            let counter = if phase / CYCLES_PER_LINE >= 160 {
                0
            } else {
                (next() & 15) as u8
            };
            let mut expected = counter;
            let mut resets = 0;
            for boundary in u64::from(phase / CYCLES_PER_LINE) + 1
                ..=(u64::from(phase) + u64::from(cycles)) / u64::from(CYCLES_PER_LINE)
            {
                let row = boundary % 228;
                if row == 0 {
                    expected = 0;
                    resets = 0;
                } else if row <= 160 {
                    let value = u32::from(expected) + 1;
                    expected = if row == 160 || value == height {
                        0
                    } else {
                        (value & 15) as u8
                    };
                    resets += u32::from(expected == 0);
                }
            }
            let p = progress(phase, cycles, counter, height);
            assert_eq!(p.counter, expected, "phase={phase} cycles={cycles}");
            assert_eq!(p.resets, resets);
        }
    }

    #[test]
    fn size_shrink_preserves_counter_until_four_bit_wrap_and_zero_cycles_do_nothing() {
        let mut mosaic = VerticalMosaic::default();
        mosaic.advance(0, 6 * CYCLES_PER_LINE, 0x3070); // BG8, OBJ4.
        assert_eq!((mosaic.background, mosaic.object), (6, 3)); // OBJ prepares row7.
        mosaic.advance(6 * CYCLES_PER_LINE, 0, 0);
        assert_eq!((mosaic.background, mosaic.object), (6, 3));
        mosaic.advance(6 * CYCLES_PER_LINE, 9 * CYCLES_PER_LINE, 0x1010); // BG2, OBJ2.
        assert_eq!((mosaic.background, mosaic.object), (15, 12));
        mosaic.advance(15 * CYCLES_PER_LINE, CYCLES_PER_LINE, 0x1010);
        assert_eq!((mosaic.background, mosaic.object), (0, 13));
        mosaic.advance(16 * CYCLES_PER_LINE, 144 * CYCLES_PER_LINE, 0xffff);
        assert_eq!((mosaic.background, mosaic.object), (0, 0));
        mosaic.advance(160 * CYCLES_PER_LINE, 68 * CYCLES_PER_LINE, 0xffff);
        assert_eq!((mosaic.background, mosaic.object), (0, 1)); // Row1 phase, row0 already prepared.
    }
}
