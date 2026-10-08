//! Regular and affine OBJ sampling with a nominal per-row work allowance.
//! Individual fetch timing and OAM contention are not modeled.

mod budget;
pub(crate) mod pipeline;

use super::{halfword, Pixel, VideoError, HEIGHT, WIDTH};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

const SIZES: [[(usize, usize); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

pub(super) struct Objects {
    pub pixels: Vec<Option<Pixel>>,
    pub window: Vec<bool>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PreparedObjects {
    samples: Vec<Sample>,
    window: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectPixel {
    palette_index: u16,
    semi_transparent: bool,
}

// Transparent texels can change priority and mosaic without replacing color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

impl ObjectPixel {
    fn save_fields(palette_index: &u16, semi_transparent: &bool, writer: &mut Writer) {
        if *palette_index > 255 {
            writer.fail(SnapshotError::Unsupported("invalid OBJ palette index"));
        }
        writer.u16(*palette_index);
        writer.bool(*semi_transparent);
    }

    fn load_fields(
        palette_index: &mut u16,
        semi_transparent: &mut bool,
        reader: &mut Reader<'_>,
    ) -> Result<(), SnapshotError> {
        let loaded_index = reader.u16()?;
        let loaded_transparency = reader.bool()?;
        if loaded_index > 255 {
            return Err(SnapshotError::Corrupt("OBJ palette index"));
        }
        *palette_index = loaded_index;
        *semi_transparent = loaded_transparency;
        Ok(())
    }

    fn save_payload(&self, writer: &mut Writer) {
        let Self {
            palette_index,
            semi_transparent,
        } = self;
        Self::save_fields(palette_index, semi_transparent, writer);
    }

    fn load_payload(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            palette_index,
            semi_transparent,
        } = self;
        Self::load_fields(palette_index, semi_transparent, reader)
    }
}

impl Snapshot for ObjectPixel {
    fn save(&self, writer: &mut Writer) {
        let Self {
            palette_index,
            semi_transparent,
        } = self;
        writer.tag(b"OPXL");
        Self::save_fields(palette_index, semi_transparent, writer);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            palette_index,
            semi_transparent,
        } = self;
        reader.tag(b"OPXL")?;
        Self::load_fields(palette_index, semi_transparent, reader)
    }
}

impl Sample {
    fn save_fields(pixel: &Option<ObjectPixel>, priority: &u8, mosaic: &bool, writer: &mut Writer) {
        writer.option(pixel.as_ref(), |pixel, writer| {
            pixel.save_payload(writer);
        });
        if *priority > 3 {
            writer.fail(SnapshotError::Unsupported("invalid OBJ priority"));
        }
        writer.u8(*priority);
        writer.bool(*mosaic);
    }

    fn load_fields(
        pixel: &mut Option<ObjectPixel>,
        priority: &mut u8,
        mosaic: &mut bool,
        reader: &mut Reader<'_>,
    ) -> Result<(), SnapshotError> {
        let loaded_pixel = reader.option(|reader| {
            let mut pixel = ObjectPixel {
                palette_index: 0,
                semi_transparent: false,
            };
            pixel.load_payload(reader)?;
            Ok(pixel)
        })?;
        let loaded_priority = reader.u8()?;
        if loaded_priority > 3 {
            return Err(SnapshotError::Corrupt("OBJ priority"));
        }
        let loaded_mosaic = reader.bool()?;
        *pixel = loaded_pixel;
        *priority = loaded_priority;
        *mosaic = loaded_mosaic;
        Ok(())
    }

    fn save_payload(&self, writer: &mut Writer) {
        let Self {
            pixel,
            priority,
            mosaic,
        } = self;
        Self::save_fields(pixel, priority, mosaic, writer);
    }

    fn load_payload(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            pixel,
            priority,
            mosaic,
        } = self;
        Self::load_fields(pixel, priority, mosaic, reader)
    }
}

impl Snapshot for Sample {
    fn save(&self, writer: &mut Writer) {
        let Self {
            pixel,
            priority,
            mosaic,
        } = self;
        writer.tag(b"SAMP");
        Self::save_fields(pixel, priority, mosaic, writer);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            pixel,
            priority,
            mosaic,
        } = self;
        reader.tag(b"SAMP")?;
        Self::load_fields(pixel, priority, mosaic, reader)
    }
}

impl Snapshot for PreparedObjects {
    fn save(&self, writer: &mut Writer) {
        let Self { samples, window } = self;
        writer.tag(b"POBJ");
        if samples.len() != window.len()
            || samples.len() % WIDTH != 0
            || samples.len() > WIDTH * HEIGHT
        {
            writer.fail(SnapshotError::Unsupported(
                "invalid prepared OBJ dimensions",
            ));
        }
        writer.u64(samples.len() as u64);
        writer.u64(window.len() as u64);
        for sample in samples {
            sample.save_payload(writer);
        }
        for value in window {
            writer.bool(*value);
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { samples, window } = self;
        reader.tag(b"POBJ")?;
        let sample_len = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("prepared OBJ dimensions"))?;
        let window_len = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("prepared OBJ dimensions"))?;
        if sample_len != window_len || sample_len % WIDTH != 0 || sample_len > WIDTH * HEIGHT {
            return Err(SnapshotError::Corrupt("prepared OBJ dimensions"));
        }
        samples.resize(sample_len, Sample::default());
        window.resize(window_len, false);
        for sample in samples {
            sample.load_payload(reader)?;
        }
        for value in window {
            *value = reader.bool()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encoded<T: Snapshot>(value: &T) -> Vec<u8> {
        let mut writer = Writer::new();
        value.save(&mut writer);
        writer.finish().unwrap()
    }

    fn assert_prefixes<T: Snapshot>(bytes: &[u8], mut fresh: impl FnMut() -> T) {
        for end in 0..bytes.len() {
            assert!(
                fresh().load(&mut Reader::new(&bytes[..end])).is_err(),
                "prefix {end}"
            );
        }
    }

    #[test]
    fn object_pixel_round_trips_and_rejects_invalid_boolean() {
        let source = ObjectPixel {
            palette_index: 219,
            semi_transparent: true,
        };
        let bytes = encoded(&source);
        let mut restored = ObjectPixel {
            palette_index: 0,
            semi_transparent: false,
        };
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
        assert_prefixes(&bytes, || ObjectPixel {
            palette_index: 0,
            semi_transparent: false,
        });
        let mut invalid = bytes.clone();
        invalid[6] = 2;
        assert_eq!(
            ObjectPixel {
                palette_index: 0,
                semi_transparent: false,
            }
            .load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("boolean"))
        );
        invalid[4] = 0xff;
        invalid[5] = 0x01;
        invalid[6] = 1;
        assert_eq!(
            ObjectPixel {
                palette_index: 0,
                semi_transparent: false,
            }
            .load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("OBJ palette index"))
        );
    }

    #[test]
    fn sample_round_trips_and_validates_priority() {
        let source = Sample {
            pixel: Some(ObjectPixel {
                palette_index: 173,
                semi_transparent: true,
            }),
            priority: 3,
            mosaic: true,
        };
        let bytes = encoded(&source);
        let mut restored = Sample::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
        assert_prefixes(&bytes, Sample::default);
        let mut invalid = bytes.clone();
        invalid[8] = 4;
        assert_eq!(
            Sample::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("OBJ priority"))
        );
        invalid[4] = 2;
        assert_eq!(
            Sample::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("boolean"))
        );
    }

    #[test]
    fn prepared_objects_round_trip_and_bound_lengths() {
        let samples = (0..WIDTH * 2)
            .map(|index| Sample {
                pixel: (index % 2 == 0).then_some(ObjectPixel {
                    palette_index: (index % 256) as u16,
                    semi_transparent: index % 3 == 0,
                }),
                priority: (index % 4) as u8,
                mosaic: index % 5 == 0,
            })
            .collect();
        let window = (0..WIDTH * 2).map(|index| index % 7 == 0).collect();
        let source = PreparedObjects { samples, window };
        let bytes = encoded(&source);
        let mut restored = PreparedObjects::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
        assert_prefixes(&bytes, PreparedObjects::default);
        let mut invalid = bytes;
        invalid[4] = 1;
        invalid[5..8].fill(0);
        assert_eq!(
            PreparedObjects::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("prepared OBJ dimensions"))
        );
        let mut invalid = encoded(&source);
        invalid[20] = 2;
        assert_eq!(
            PreparedObjects::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("boolean"))
        );
    }
}
