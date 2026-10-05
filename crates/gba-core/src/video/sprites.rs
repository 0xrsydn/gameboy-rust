//! Regular and affine OBJ sampling with a nominal per-row work allowance.
//! Individual fetch timing and OAM contention are not modeled.

mod budget;
pub(crate) mod pipeline;

use super::{halfword, Pixel, VideoError, WIDTH};

const SIZES: [[(usize, usize); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

pub(super) struct Objects {
    pub pixels: Vec<Option<Pixel>>,
    pub window: Vec<bool>,
}

pub(crate) struct PreparedObjects {
    samples: Vec<Sample>,
    window: Vec<bool>,
}

#[derive(Clone, Copy)]
struct ObjectPixel {
    palette_index: u16,
    semi_transparent: bool,
}

// Transparent texels can change priority and mosaic without replacing color.
#[derive(Clone, Copy, Default)]
struct Sample {
    pixel: Option<ObjectPixel>,
    priority: u8,
    mosaic: bool,
}

pub(crate) fn prepare(
    control: u16,
    mosaic: u16,
    vertical_counter: Option<u8>,
    oam: &[u8],
    vram: &[u8],
    rows: std::ops::Range<usize>,
) -> Result<PreparedObjects, VideoError> {
    // Lower priority numbers win; equal priorities retain the earlier OAM entry.
    // Color, priority, and mosaic metadata are tracked separately.
    let mut output = PreparedObjects {
        samples: vec![Sample::default(); WIDTH * rows.len()],
        window: vec![false; WIDTH * rows.len()],
    };
    if control & 0x1000 == 0 {
        return Ok(output);
    }
    let samples = &mut output.samples;
    let mut budgets = vec![budget::Budget::new(control); rows.len()];
    let mosaic_height = usize::from(mosaic >> 12) + 1;
    let one_dimensional = control & 0x40 != 0;
    for index in 0..128 {
        let a = usize::from(halfword(oam, index * 8));
        let b = usize::from(halfword(oam, index * 8 + 2));
        let c = usize::from(halfword(oam, index * 8 + 4));
        if a & 0x300 == 0x200 {
            for budget in &mut budgets {
                budget.skip();
            }
            continue; // Disabled regular OBJ still consumes nominal OAM inspection.
        }
        let reason = if a & 0xc00 == 0xc00 {
            Some("prohibited OBJ mode")
        } else if a >> 14 == 3 {
            Some("prohibited OBJ shape")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(VideoError::UnsupportedObject { index, reason });
        }
        let window = a & 0xc00 == 0x800;
        // Window coverage is prepared independently of its later display mask.
        // OBJ-window coverage does not use either mosaic dimension.
        let mosaic_enabled = a & 0x1000 != 0 && !window;
        let (width, height) = SIZES[a >> 14][b >> 14];
        let affine = a & 0x100 != 0;
        let doubled = affine && a & 0x200 != 0;
        let canvas_width = width << usize::from(doubled);
        let canvas_height = height << usize::from(doubled);
        let matrix = if affine {
            let base = ((b >> 9) & 31) * 32 + 6;
            [0, 8, 16, 24].map(|offset| i32::from(halfword(oam, base + offset) as i16))
        } else {
            [0; 4]
        };
        let eight_bit = a & 0x2000 != 0;
        let mut tile = c & 0x3ff;
        // Restricted bitmap tiles produce no pixels, but active sprite work
        // still uses the allowance. They are not disabled OAM entries.
        let tile_visible = !(control & 7 >= 3 && tile < 512);
        if eight_bit && !one_dimensional {
            tile &= !1; // 2D 8bpp tiles are aligned to pairs of 32-byte slots.
        }
        let slot_width = if eight_bit { 2 } else { 1 };
        let priority = ((c >> 10) & 3) as u8;
        for y in rows.clone() {
            let budget = &mut budgets[y - rows.start];
            let sy = (y + 256 - (a & 255)) & 255;
            let left = b & 511;
            let intersects_screen = left < WIDTH || left + canvas_width > 512;
            if sy >= canvas_height || !intersects_screen {
                budget.skip();
                continue;
            }
            // Charge before texel lookup or priority/window masking. Transparent,
            // hidden, and out-of-texture samples still require preparation work.
            let columns = budget.draw(canvas_width, affine);
            if !tile_visible {
                continue;
            }
            // Preparation latches the ahead-of-display OBJ counter; snapshots
            // use the screen grid.
            // Clamp partial first blocks to the object's first row, without
            // extending its canvas. OBJ-window sampling bypasses this counter.
            let sample_y = if mosaic_enabled {
                sy.saturating_sub(vertical_counter.map_or(y % mosaic_height, usize::from))
            } else {
                sy
            };
            for sx in 0..columns {
                let x = ((b & 511) + sx) & 511;
                if x >= WIDTH {
                    continue;
                }
                let offset = (y - rows.start) * WIDTH + x;
                let (tx, ty) = if affine {
                    // Signed 8.8 inverse mapping from canvas center to texture center.
                    // Keep fractional products until the final arithmetic shift. Negative
                    // coordinates round down, not toward zero. Outside texels are transparent.
                    let dx = sx as i32 - canvas_width as i32 / 2;
                    let dy = sample_y as i32 - canvas_height as i32 / 2;
                    let tx = (matrix[0] * dx + matrix[1] * dy + ((width as i32) << 7)) >> 8;
                    let ty = (matrix[2] * dx + matrix[3] * dy + ((height as i32) << 7)) >> 8;
                    if tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
                        continue;
                    }
                    (tx as usize, ty as usize)
                } else {
                    (
                        if b & 0x1000 != 0 { width - 1 - sx } else { sx },
                        if b & 0x2000 != 0 {
                            height - 1 - sample_y
                        } else {
                            sample_y
                        },
                    )
                };
                let slot = if one_dimensional {
                    tile + (ty / 8 * (width / 8) + tx / 8) * slot_width
                } else {
                    // A 2D row is 32 slots wide. Horizontal overflow wraps within
                    // that row; vertical overflow wraps within 32 KiB OBJ VRAM.
                    (tile & !31) + (ty / 8) * 32 + ((tile + tx / 8 * slot_width) & 31)
                };
                let pixel = (ty % 8) * 8 + tx % 8;
                let address =
                    0x10000 + ((slot * 32 + if eight_bit { pixel } else { pixel / 2 }) & 0x7fff);
                let color_index = if eight_bit {
                    usize::from(vram[address])
                } else {
                    usize::from(vram[address] >> ((pixel & 1) * 4) & 15)
                };
                if window && color_index != 0 {
                    output.window[offset] = true;
                } else {
                    let sample = &mut samples[offset];
                    if sample.pixel.is_none() || priority < sample.priority {
                        if color_index != 0 {
                            let bank = if eight_bit { 0 } else { (c >> 12) * 16 };
                            sample.pixel = Some(ObjectPixel {
                                palette_index: (bank + color_index) as u16,
                                semi_transparent: a & 0xc00 == 0x400,
                            });
                        }
                        sample.priority = priority;
                        sample.mosaic = mosaic_enabled;
                    }
                }
            }
        }
    }
    Ok(output)
}

impl PreparedObjects {
    /// Resolve buffered indices through the current palette. Horizontal mosaic
    /// belongs to composition, not the previous line's OAM/VRAM preparation.
    pub(super) fn compose(&self, mosaic: u16, palette: &[u8]) -> Objects {
        let mut output = Objects {
            pixels: vec![None; self.samples.len()],
            window: self.window.clone(),
        };
        let mosaic_width = usize::from((mosaic >> 8) & 15) + 1;
        // Horizontal OBJ mosaic latches the selected object sample, not each
        // object's texture coordinate. Reset at each scanline and at transitions
        // to/from non-mosaic samples or a numerically lower priority.
        for (source, destination) in self
            .samples
            .chunks_exact(WIDTH)
            .zip(output.pixels.chunks_exact_mut(WIDTH))
        {
            let mut latched = Sample::default();
            for (x, (&current, pixel)) in source.iter().zip(destination).enumerate() {
                if x % mosaic_width == 0
                    || !current.mosaic
                    || !latched.mosaic
                    || current.priority < latched.priority
                {
                    latched = current;
                }
                *pixel = latched.pixel.map(|p| Pixel {
                    color: halfword(palette, 0x200 + usize::from(p.palette_index) * 2),
                    layer: 4,
                    priority: latched.priority,
                    semi_transparent: p.semi_transparent,
                });
            }
        }
        output
    }
}
