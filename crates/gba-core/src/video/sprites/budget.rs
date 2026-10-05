//! Nominal per-row OBJ work allowance, not a timed OAM/VRAM fetch scheduler.
//! GBATEK gives 1210/954 cycles and costs of width or 10+2*width.
//! Inactive entries cost two inspection cycles, following emulator references.
//! Active costs include that inspection; clipped active canvases cost full width.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Budget {
    remaining: usize,
}

impl Budget {
    pub fn new(control: u16) -> Self {
        Self {
            remaining: if control & 0x20 != 0 { 954 } else { 1210 },
        }
    }

    pub fn skip(&mut self) {
        self.remaining = self.remaining.saturating_sub(2);
    }

    /// Return the prepared prefix of the drawing canvas. Unfinished work consumes
    /// the remaining allowance; it cannot be reused by a later, cheaper object.
    pub fn draw(&mut self, width: usize, affine: bool) -> usize {
        if self.remaining < 2 {
            self.remaining = 0;
            return 0;
        }
        let (setup, per_pixel) = if affine { (10, 2) } else { (0, 1) };
        let pixels = (self.remaining.saturating_sub(setup) / per_pixel).min(width);
        self.remaining = self.remaining.saturating_sub(setup + per_pixel * width);
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_allowance_and_width_matches_independent_cycle_consumption() {
        for initial in 0usize..=1210 {
            for width in [8, 16, 32, 64, 128] {
                for affine in [false, true] {
                    let mut expected = initial;
                    let mut pixels = 0;
                    if expected < 2 {
                        expected = 0;
                    } else {
                        if affine {
                            for _ in 0..10 {
                                expected = expected.saturating_sub(1);
                            }
                        }
                        for _ in 0..width {
                            let needed = if affine { 2 } else { 1 };
                            if expected < needed {
                                expected = 0;
                                break;
                            }
                            expected -= needed;
                            pixels += 1;
                        }
                    }
                    let mut budget = Budget { remaining: initial };
                    assert_eq!(
                        budget.draw(width, affine),
                        pixels,
                        "initial={initial} width={width} affine={affine}"
                    );
                    assert_eq!(budget.remaining, expected);
                }
            }
        }
    }

    #[test]
    fn inactive_scan_and_active_costs_do_not_double_charge_inspection() {
        let mut normal = Budget::new(0);
        assert_eq!(normal.draw(64, false), 64);
        assert_eq!(normal.remaining, 1210 - 64);
        normal.skip();
        assert_eq!(normal.remaining, 1210 - 66);
        assert_eq!(normal.draw(128, true), 128);
        assert_eq!(normal.remaining, 1210 - 66 - 266);
        let mut short = Budget::new(0x20);
        for _ in 0..119 {
            assert_eq!(short.draw(8, false), 8);
        }
        assert_eq!(short.draw(8, false), 2);
        assert_eq!(short.draw(8, false), 0);
    }

    #[test]
    fn random_mixed_work_matches_an_independent_remaining_time_model() {
        let mut seed = 0x8321_602bu32;
        for control in [0, 0x20] {
            for _ in 0..1000 {
                let mut budget = Budget::new(control);
                let mut expected: i32 = if control == 0 { 1210 } else { 954 };
                for _ in 0..128 {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let width = 8 << ((seed >> 4) % 5);
                    let affine = seed & 8 != 0;
                    if seed & 3 == 0 {
                        budget.skip();
                        expected = (expected - 2).max(0);
                    } else {
                        let columns = if expected < 2 {
                            0
                        } else if affine {
                            ((expected - 10).max(0) / 2).min(width)
                        } else {
                            expected.min(width)
                        };
                        assert_eq!(budget.draw(width as usize, affine), columns as usize);
                        expected = (expected - if affine { 10 + 2 * width } else { width }).max(0);
                    }
                    assert_eq!(budget.remaining, expected as usize);
                }
            }
        }
    }
}
