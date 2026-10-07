//! Programmed affine BG registers and sampling for snapshots and row capture.
//! Internal origins advance at line boundaries. Pixel-fetch and per-access timing
//! remain outside this row-level model.

use super::{halfword, vram_index, Background, VideoError, HEIGHT, WIDTH};
#[cfg(test)]
use crate::display::{CYCLES_PER_FRAME, CYCLES_PER_LINE};

const REFERENCE_MASK: u32 = 0x0fff_ffff;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AffineBackground {
    matrix: [u16; 4],
    reference: [u32; 2],
    internal: [u32; 2],
    // An HBlank write sets the next row's origin and overrides that row's increment.
    reload_next: [bool; 2],
}

impl AffineBackground {
    /// Merge into write-only latches, never into their zero readback placeholders.
    pub(crate) fn write8(&mut self, offset: usize, value: u8, after_row: bool) {
        if offset < 8 {
            let word = &mut self.matrix[offset / 2];
            let shift = (offset & 1) * 8;
            *word = (*word & !(0xff << shift)) | (u16::from(value) << shift);
        } else {
            let axis = (offset - 8) / 4;
            let word = &mut self.reference[axis];
            let shift = (offset & 3) * 8;
            *word = ((*word & !(0xff << shift)) | (u32::from(value) << shift)) & REFERENCE_MASK;
            self.internal[axis] = *word;
            self.reload_next[axis] = after_row;
        }
    }

    /// Advance across line ends with fixed registers. Reload at the next visible
    /// frame's line-zero boundary, so arbitrarily large batches remain O(1).
    pub(crate) fn advance(
        &mut self,
        phase: u32,
        cycles: u32,
        active: bool,
        mosaic_height: u32,
        counter: u8,
    ) {
        let progress = super::mosaic::progress(phase, cycles, counter, mosaic_height);
        if progress.frame_reloaded {
            self.internal = self.reference;
            self.reload_next = [false; 2];
        }
        if !progress.crossed_line {
            return;
        }
        // A mosaic-enabled affine origin advances only when the shared BG
        // counter becomes zero. Height1/counter0 models non-mosaic backgrounds.
        for (axis, coefficient) in [self.matrix[1], self.matrix[3]].into_iter().enumerate() {
            if active {
                let count =
                    progress.resets - u32::from(self.reload_next[axis] && progress.first_reset);
                let delta = i32::from(coefficient as i16) * (count * mosaic_height) as i32;
                self.internal[axis] =
                    self.internal[axis].wrapping_add(delta as u32) & REFERENCE_MASK;
            }
            self.reload_next[axis] = false;
        }
    }

    fn source(self, x: usize, y: usize, internal: bool) -> (i32, i32) {
        let [a, b, c, d] = self.matrix.map(|v| i32::from(v as i16));
        let reference = if internal {
            self.internal
        } else {
            self.reference
        };
        let [rx, ry] = reference.map(|v| ((v << 4) as i32) >> 4);
        // Internal origins already include vertical progress.
        let y = if internal { 0 } else { y };
        // Products and sums fit i32 for a 240x160 output. Model the 28-bit
        // fixed-point range, then floor with an arithmetic shift by eight.
        let coordinate = |v: i32| ((v as u32) << 4) as i32 >> 12;
        (
            coordinate(rx + a * x as i32 + b * y as i32),
            coordinate(ry + c * x as i32 + d * y as i32),
        )
    }
}

pub(super) fn pixel(
    bg: Background,
    vram: &[u8],
    palette: &[u8],
    x: usize,
    y: usize,
    display_control: u16,
    internal: bool,
) -> Result<Option<u16>, VideoError> {
    let (mut tx, mut ty) = bg.affine.source(x, y, internal);
    let mode = display_control & 7;
    if (3..=5).contains(&mode) {
        let (width, height) = if mode == 5 {
            (160, 128)
        } else {
            (WIDTH, HEIGHT)
        };
        // Bitmap overflow never wraps, even when BGCNT bit 13 is set.
        if tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
            return Ok(None);
        }
        // Mode 3 ignores the page bit. Modes 4/5 share the same 40 KiB page stride.
        let page = if mode != 3 && display_control & 0x10 != 0 {
            0xa000
        } else {
            0
        };
        let offset = ty as usize * width + tx as usize;
        return Ok(if mode == 4 {
            let index = usize::from(vram[page + offset]);
            if index == 0 {
                None
            } else {
                Some(halfword(palette, index * 2))
            }
        } else {
            // Modes 3/5 store opaque RGB555, including black. Bit 15 is unused.
            Some(halfword(vram, page + offset * 2))
        });
    }
    let control = usize::from(bg.control);
    let size = 128_i32 << (control >> 14);
    if control & 0x2000 != 0 {
        tx = tx.rem_euclid(size);
        ty = ty.rem_euclid(size);
    } else if tx < 0 || ty < 0 || tx >= size || ty >= size {
        return Ok(None);
    }
    let (tx, ty) = (tx as usize, ty as usize);
    let map = ((control >> 8) & 31) * 0x800 + (ty / 8) * (size as usize / 8) + tx / 8;
    if map >= 0x20000 {
        return Err(VideoError::UnsupportedMapAddress(map));
    }
    // Affine maps have byte-sized indices and no flip/palette-bank attributes.
    // Every tile is 8bpp, regardless of BGCNT bit 7.
    let address = vram_index(map);
    let address =
        ((control >> 2) & 3) * 0x4000 + usize::from(vram[address]) * 64 + (ty % 8) * 8 + tx % 8;
    if address >= 0x20000 {
        return Err(VideoError::UnsupportedTileAddress(address));
    }
    let index = usize::from(vram[vram_index(address)]);
    Ok(if index == 0 {
        None
    } else {
        Some(halfword(palette, index * 2))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_tracking_matches_an_independent_line_boundary_model() {
        let mut seed = 0x92ad_1037u32;
        for _ in 0..500 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            };
            let mut bg = AffineBackground {
                matrix: [next() as u16, next() as u16, next() as u16, next() as u16],
                reference: [next() & REFERENCE_MASK, next() & REFERENCE_MASK],
                internal: [next() & REFERENCE_MASK, next() & REFERENCE_MASK],
                reload_next: [next() & 1 != 0, next() & 2 != 0],
            };
            let phase = next() % CYCLES_PER_FRAME;
            let cycles = next() % (CYCLES_PER_FRAME * 7);
            let height = next() % 16 + 1;
            let counter = (next() & 15) as u8;
            let mut expected_counter = counter;
            let active = next() & 8 != 0;
            let mut expected = bg.internal.map(i64::from);
            let mut pending = bg.reload_next;
            let first = phase / CYCLES_PER_LINE + 1;
            let last = (phase + cycles) / CYCLES_PER_LINE;
            for boundary in first..=last {
                let row = boundary % 228;
                if row == 0 {
                    expected = bg.reference.map(i64::from);
                    expected_counter = 0;
                } else if row <= 160 {
                    let next = u32::from(expected_counter) + 1;
                    expected_counter = if row == 160 || next == height {
                        0
                    } else {
                        (next & 15) as u8
                    };
                    for axis in 0..2 {
                        if active && expected_counter == 0 && !pending[axis] {
                            let coefficient = i64::from(bg.matrix[axis * 2 + 1] as i16);
                            expected[axis] = (expected[axis] + coefficient * i64::from(height))
                                .rem_euclid(1 << 28);
                        }
                    }
                }
                pending = [false; 2];
            }
            bg.advance(phase, cycles, active, height, counter);
            assert_eq!(
                bg.internal.map(i64::from),
                expected,
                "phase={phase} cycles={cycles} height={height} active={active}"
            );
            assert_eq!(bg.reload_next, pending);
        }
    }

    #[test]
    fn signed_accumulators_wrap_and_extreme_batches_reload_without_iteration() {
        let mut bg = AffineBackground {
            matrix: [256, 1, 0, (-1i16) as u16],
            reference: [REFERENCE_MASK, 0],
            internal: [REFERENCE_MASK, 0],
            ..Default::default()
        };
        bg.advance(0, CYCLES_PER_LINE, true, 1, 0);
        assert_eq!(bg.internal, [0, REFERENCE_MASK]);
        assert_eq!(bg.source(0, 0, true), (0, -1));
        assert_eq!(bg.source(0, 0, false), (-1, 0));
        let phase = 17 * CYCLES_PER_LINE + 413;
        bg.advance(phase, u32::MAX, true, 1, 0);
        let lines = ((u64::from(phase) + u64::from(u32::MAX)) % u64::from(CYCLES_PER_FRAME)
            / u64::from(CYCLES_PER_LINE))
        .min(160);
        assert_eq!(
            bg.internal[0],
            ((i64::from(REFERENCE_MASK) + lines as i64) % (1 << 28)) as u32
        );
        assert_eq!(bg.internal[1], (-(lines as i64)).rem_euclid(1 << 28) as u32);
    }

    #[test]
    fn reference_byte_writes_merge_programmed_values_not_accumulated_values() {
        let mut bg = AffineBackground {
            matrix: [256, 256, 0, 256],
            reference: [0x1234500, 0x2000],
            internal: [0x1234500, 0x2000],
            ..Default::default()
        };
        bg.advance(0, 10 * CYCLES_PER_LINE, true, 1, 0);
        let y = bg.internal[1];
        bg.write8(8, 0x7f, false);
        assert_eq!(bg.reference[0], 0x123457f);
        assert_eq!(bg.internal[0], 0x123457f);
        assert_eq!(bg.internal[1], y);
        bg.write8(11, 0xff, true);
        assert_eq!(bg.reference[0], 0x0f23457f);
        bg.advance(
            10 * CYCLES_PER_LINE + 1006,
            CYCLES_PER_LINE - 1006,
            true,
            1,
            0,
        );
        assert_eq!(bg.internal[0], 0x0f23457f); // HBlank reload overrides this transition.
        assert_eq!(bg.internal[1], y + 256); // Other axis still increments.
        bg.write8(2, 0, false); // PB byte write must not reload X.
        assert_eq!(bg.internal[0], 0x0f23457f);
    }
}
