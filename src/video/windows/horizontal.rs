//! Horizontal comparator history only, not pixel-fetch or color-register timing.
//! Sample X=0..255 at line cycles 0,4,..1020; right-edge matches clear after left.

use crate::{display::CYCLES_PER_LINE, video::WIDTH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HorizontalWindows {
    active: [bool; 2],
    pub pixels: [[bool; WIDTH]; 2],
}

impl Default for HorizontalWindows {
    fn default() -> Self {
        Self {
            active: [false; 2],
            pixels: [[false; WIDTH]; 2],
        }
    }
}

impl HorizontalWindows {
    /// Consume the half-open clock interval [phase, phase+cycles). A write at
    /// cycle 4*x therefore precedes X's comparison; a write at 4*x+1 is too late.
    /// Process at most three line segments, even across arbitrarily many frames.
    pub fn advance(&mut self, phase: u32, cycles: u32, bounds: [u16; 2]) {
        if cycles == 0 {
            return;
        }
        let start = phase % CYCLES_PER_LINE;
        let end = u64::from(start) + u64::from(cycles);
        let last_line = (end - 1) / u64::from(CYCLES_PER_LINE);
        if last_line == 0 {
            self.segment(start, end as u32, bounds);
            return;
        }
        self.segment(start, CYCLES_PER_LINE, bounds);
        if last_line > 1 {
            if last_line > 2 {
                // Two or more full intervening lines: both edges have fired,
                // so the last full line starts in the stable wrapping state.
                self.active = bounds.map(|b| (b >> 8) > (b & 255));
            }
            // Retain unvisited pixels from the previous line, just as repeated
            // single-cycle advancement does. Never clear history on line entry.
            self.segment(0, CYCLES_PER_LINE, bounds);
        }
        let tail = ((end - 1) % u64::from(CYCLES_PER_LINE) + 1) as u32;
        self.segment(0, tail, bounds);
    }

    fn segment(&mut self, start: u32, end: u32, bounds: [u16; 2]) {
        let first = start.div_ceil(4).min(256) as usize;
        let last = end.div_ceil(4).min(256) as usize;
        if first == last {
            return;
        }
        for (window, bounds) in bounds.into_iter().enumerate() {
            let left = usize::from(bounds >> 8);
            let right = usize::from(bounds & 255);
            let edges = if left <= right {
                [(left, true), (right, false)]
            } else {
                [(right, false), (left, true)]
            };
            let mut from = first;
            for (edge, state) in edges {
                if (first..last).contains(&edge) {
                    self.pixels[window][from.min(WIDTH)..edge.min(WIDTH)].fill(self.active[window]);
                    self.active[window] = state;
                    from = edge;
                }
            }
            self.pixels[window][from.min(WIDTH)..last.min(WIDTH)].fill(self.active[window]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(state: &mut HorizontalWindows, start: u32, cycles: u32, bounds: [u16; 2]) {
        // Independent dot iteration, including offscreen columns and hidden lines.
        let end = u64::from(start) + u64::from(cycles);
        let mut tick = u64::from(start.div_ceil(4)) * 4;
        while tick < end {
            let x = (tick % u64::from(CYCLES_PER_LINE)) / 4;
            if x < 256 {
                for (window, bounds) in bounds.into_iter().enumerate() {
                    if x == u64::from(bounds >> 8) {
                        state.active[window] = true;
                    }
                    if x == u64::from(bounds & 255) {
                        state.active[window] = false;
                    }
                    if x < WIDTH as u64 {
                        state.pixels[window][x as usize] = state.active[window];
                    }
                }
            }
            tick += 4;
        }
    }

    #[test]
    fn all_bounds_match_dot_iteration_for_startup_and_wrapped_next_line() {
        for bounds in 0..=u16::MAX {
            let mut actual = HorizontalWindows::default();
            let mut expected = actual.clone();
            for phase in [0, CYCLES_PER_LINE] {
                actual.advance(phase, CYCLES_PER_LINE, [bounds; 2]);
                reference(&mut expected, phase, CYCLES_PER_LINE, [bounds; 2]);
                assert_eq!(actual, expected, "bounds={bounds:#x} phase={phase}");
            }
        }
    }

    #[test]
    fn arbitrary_partial_lines_and_bound_writes_match_dot_iteration() {
        let mut seed = 0x371b_c321u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        let mut actual = HorizontalWindows::default();
        let mut expected = actual.clone();
        let mut phase = 0;
        for _ in 0..2000 {
            let bounds = [next() as u16, next() as u16];
            let cycles = next() % (9 * CYCLES_PER_LINE);
            actual.advance(phase, cycles, bounds);
            reference(&mut expected, phase, cycles, bounds);
            assert_eq!(
                actual, expected,
                "phase={phase} cycles={cycles} bounds={bounds:?}"
            );
            phase = (phase + cycles) % (228 * CYCLES_PER_LINE);
        }
    }

    #[test]
    fn largest_batch_has_same_state_as_short_batch_with_same_final_phase() {
        for start in [0, 1, 79, 955, 1006, 1021, 1231] {
            let mut actual = HorizontalWindows::default();
            let mut expected = actual.clone();
            let cycles = u32::MAX;
            // After several complete fixed-bound lines all earlier history is replaced.
            let short = 4 * CYCLES_PER_LINE + cycles % CYCLES_PER_LINE;
            actual.advance(start, cycles, [0xf00a, 0x1428]);
            reference(&mut expected, start, short, [0xf00a, 0x1428]);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn zero_cycles_do_not_sample_or_replay_edges() {
        let mut state = HorizontalWindows::default();
        state.advance(0, 0, [0x0010; 2]);
        assert_eq!(state, HorizontalWindows::default());
        state.advance(0, 1, [0x0010; 2]);
        assert!(state.active[0] && state.pixels[0][0]);
        let before = state.clone();
        state.advance(1, 0, [0; 2]);
        assert_eq!(state, before);
        state.advance(1, 3, [0; 2]);
        assert_eq!(state, before); // X0 already passed, X1 has not started.
    }
}
