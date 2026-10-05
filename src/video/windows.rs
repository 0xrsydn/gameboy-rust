//! Vertical WIN0/WIN1 comparators. State survives frame wrap and register writes.
//! Horizontal comparators retain per-column history separately from vertical state.

pub(crate) mod horizontal;

use crate::display::{CYCLES_PER_LINE, LINES_PER_FRAME};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowEdges {
    pub active: [bool; 2],
    started: bool,
}

impl WindowEdges {
    /// Evaluate each crossed line start, including hidden lines. With fixed bounds,
    /// only the latest matching edge matters, so large clock batches remain O(1).
    pub fn advance(&mut self, phase: u32, cycles: u32, bounds: [u16; 2]) {
        if cycles == 0 {
            return;
        }
        if !self.started {
            // Allow host setup before the first clock advance at reset's line zero.
            // Later writes at an already-entered line never replay its comparator.
            let row = phase / CYCLES_PER_LINE;
            for (window, bounds) in bounds.into_iter().enumerate() {
                if row == u32::from(bounds >> 8) {
                    self.active[window] = true;
                }
                if row == u32::from(bounds & 255) {
                    self.active[window] = false;
                }
            }
            self.started = true;
        }
        let first = u64::from(phase / CYCLES_PER_LINE) + 1;
        let last = (u64::from(phase) + u64::from(cycles)) / u64::from(CYCLES_PER_LINE);
        let period = u64::from(LINES_PER_FRAME);
        let latest = |row: u16| {
            if u32::from(row) >= LINES_PER_FRAME {
                return None; // VCOUNT never reaches values 228..255.
            }
            let distance = (last + period - u64::from(row)) % period;
            last.checked_sub(distance).filter(|&edge| edge >= first)
        };
        for (window, bounds) in bounds.into_iter().enumerate() {
            let top = latest(bounds >> 8);
            let bottom = latest(bounds & 255);
            match (top, bottom) {
                (Some(top), Some(bottom)) => self.active[window] = top > bottom,
                (Some(_), None) => self.active[window] = true,
                (None, Some(_)) => self.active[window] = false,
                (None, None) => {} // No edge: retain state, even across frames.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::CYCLES_PER_FRAME;

    fn step(active: &mut bool, bounds: u16, row: u32) {
        if row == u32::from(bounds >> 8) {
            *active = true;
        }
        if row == u32::from(bounds & 255) {
            *active = false;
        }
    }

    #[test]
    fn all_bounds_match_independent_edge_iteration_across_two_frames() {
        for bounds in 0..=u16::MAX {
            let mut edges = WindowEdges::default();
            let mut expected = false;
            for row in 0..=LINES_PER_FRAME * 2 {
                step(&mut expected, bounds, row % LINES_PER_FRAME);
            }
            edges.advance(0, 2 * CYCLES_PER_FRAME, [bounds; 2]);
            assert_eq!(edges.active, [expected; 2], "bounds={bounds:#06x}");
        }
    }

    #[test]
    fn random_batches_and_extreme_batch_match_independent_edge_iteration() {
        let mut seed = 0x60c3_273bu32;
        for case in 0..500 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let bounds = [next() as u16, next() as u16];
            let phase = next() % CYCLES_PER_FRAME;
            let cycles = if case == 0 {
                u32::MAX
            } else {
                next() % (7 * CYCLES_PER_FRAME)
            };
            let mut edges = WindowEdges {
                active: [next() & 4 != 0, next() & 8 != 0],
                started: true,
            };
            let mut expected = edges.active;
            for boundary in u64::from(phase / CYCLES_PER_LINE) + 1
                ..=(u64::from(phase) + u64::from(cycles)) / u64::from(CYCLES_PER_LINE)
            {
                for window in 0..2 {
                    step(
                        &mut expected[window],
                        bounds[window],
                        (boundary % u64::from(LINES_PER_FRAME)) as u32,
                    );
                }
            }
            edges.advance(phase, cycles, bounds);
            assert_eq!(edges.active, expected, "phase={phase} cycles={cycles}");
        }
    }

    #[test]
    fn zero_cycles_and_writes_do_not_replay_an_entered_line() {
        let mut edges = WindowEdges::default();
        edges.advance(0, 0, [0x0010, 0x0020]);
        assert!(!edges.started);
        edges.advance(0, 1, [0x0010, 0x0020]);
        assert_eq!(edges.active, [true; 2]);
        edges.advance(1, CYCLES_PER_LINE - 1, [0x0110, 0x0101]);
        assert_eq!(edges.active, [true, false]); // Bottom wins when edges coincide.
        edges.advance(CYCLES_PER_LINE, 1, [0x0101, 0x0110]);
        assert_eq!(edges.active, [true, false]); // The row1 comparator already ran.
    }
}
