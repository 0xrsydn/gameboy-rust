//! Platform-independent Mode 0–5 rendering for snapshots and row capture.
//! Includes window masks, mosaic, and color effects. Horizontal window history
//! has four-cycle timing. Sprites prepare one row ahead; backgrounds, palettes,
//! and color effects still sample once per displayed row.

mod affine;
pub(crate) mod capture;
pub(crate) mod effects;
pub(crate) mod mosaic;
pub(crate) mod sprites;
pub(crate) mod windows;

use std::{error::Error, fmt};

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;

/// Convert GBA RGB555 (red in bits 0..4) into packed 0x00RRGGBB.
/// Bit 15 is unused. Replication expands five-bit channels to eight bits.
pub fn rgb555_to_rgb888(color: u16) -> u32 {
    let expand = |channel: u16| u32::from((channel << 3) | (channel >> 2));
    let red = expand(color & 31);
    let green = expand((color >> 5) & 31);
    let blue = expand((color >> 10) & 31);
    (red << 16) | (green << 8) | blue
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoError {
    UnsupportedMode(u8),
    UnsupportedLayers(u16),
    UnsupportedTileAddress(usize),
    UnsupportedMapAddress(usize),
    UnsupportedObject { index: usize, reason: &'static str },
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode(mode) => write!(
                f,
                "unsupported display mode {mode}; only Modes 0 through 5 are rendered"
            ),
            Self::UnsupportedTileAddress(address) => write!(
                f,
                "background tile address {address:#x} exceeds the supported 128 KiB BG VRAM window"
            ),
            Self::UnsupportedMapAddress(address) => write!(
                f,
                "background map address {address:#x} exceeds the supported 128 KiB BG VRAM window"
            ),
            Self::UnsupportedObject { index, reason } => write!(f, "OBJ{index}: {reason}"),
            Self::UnsupportedLayers(control) => {
                write!(f, "unsupported display layers in DISPCNT {control:#06x}")
            }
        }
    }
}

impl Error for VideoError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Background {
    pub control: u16,
    pub x: u16,
    pub y: u16,
    pub affine: affine::AffineBackground,
}

pub(crate) struct RenderSettings<'a> {
    pub control: u16,
    pub prepared_objects: Option<&'a Result<sprites::PreparedObjects, VideoError>>,
    pub internal_affine: bool,
    pub vertical_windows: Option<[bool; 2]>,
    pub horizontal_windows: Option<&'a windows::horizontal::HorizontalWindows>,
    pub green_swap: bool,
    pub mosaic: u16,
    pub vertical_mosaic: Option<mosaic::VerticalMosaic>,
    pub backgrounds: &'a [Background; 4],
    pub effects: &'a effects::Effects,
}

#[derive(Clone, Copy)]
struct Pixel {
    color: u16,
    layer: u8, // BG0..3, OBJ=4, backdrop=5.
    priority: u8,
    semi_transparent: bool,
}

impl Pixel {
    fn order(self) -> (u8, u8) {
        // OBJ wins a tie against BG; lower BG number wins other ties.
        (
            self.priority,
            if self.layer == 4 { 0 } else { self.layer + 1 },
        )
    }
}

/// Snapshot the current registers and memory. Errors leave the output unchanged.
/// Memory owns fixed-size 96 KiB VRAM, 1 KiB palette, and 1 KiB OAM buffers.
pub(crate) fn render(
    settings: RenderSettings<'_>,
    vram: &[u8],
    palette: &[u8],
    oam: &[u8],
    frame: &mut Framebuffer,
) -> Result<(), VideoError> {
    frame.pixels = render_rows(settings, vram, palette, oam, 0..HEIGHT)?;
    Ok(())
}

/// Render the requested rows from current registers and optional scanline state.
/// Row capture uses prepared sprites, affine origins, window history, and mosaic counters.
/// Debug snapshots use programmed origins, geometric window bounds, and screen-grid mosaic.
pub(crate) fn render_rows(
    settings: RenderSettings<'_>,
    vram: &[u8],
    palette: &[u8],
    oam: &[u8],
    rows: std::ops::Range<usize>,
) -> Result<Vec<u32>, VideoError> {
    debug_assert!(rows.start <= rows.end && rows.end <= HEIGHT);
    let length = rows.len() * WIDTH;
    let RenderSettings {
        control,
        prepared_objects,
        internal_affine,
        vertical_windows,
        horizontal_windows,
        green_swap,
        mosaic,
        vertical_mosaic,
        backgrounds,
        effects,
    } = settings;
    if control & 0x80 != 0 {
        return Ok(vec![0xffffff; length]);
    }
    let backdrop = Pixel {
        color: halfword(palette, 0),
        layer: 5,
        priority: 4,
        semi_transparent: false,
    };
    let mut output = vec![0; length];
    let mode = control & 7;
    let unsupported = match mode {
        0 => 0,
        1 => 0x800,     // BG0/1 text, BG2 affine.
        2 => 0x300,     // BG2/3 affine.
        3..=5 => 0xb00, // BG2 bitmap.
        _ => return Err(VideoError::UnsupportedMode(mode as u8)),
    };
    if control & unsupported != 0 {
        return Err(VideoError::UnsupportedLayers(control));
    }
    let mut layers: Vec<usize> = (0..4).filter(|bg| control & (0x100 << bg) != 0).collect();
    // Lower priority number wins. BG0 wins ties, then BG1, BG2, BG3.
    layers.sort_by_key(|&bg| (backgrounds[bg].control & 3, bg));
    let objects = if control & 0x1000 == 0 {
        sprites::Objects {
            pixels: vec![None; length],
            window: vec![false; length],
        }
    } else if let Some(prepared) = prepared_objects {
        debug_assert_eq!(rows.len(), 1);
        prepared
            .as_ref()
            .map_err(Clone::clone)?
            .compose(mosaic, palette)
    } else {
        sprites::prepare(control, mosaic, None, oam, vram, rows.clone())?.compose(mosaic, palette)
    };
    let mosaic_width = usize::from(mosaic & 15) + 1;
    let mosaic_height = usize::from((mosaic >> 4) & 15) + 1;
    for y in rows.clone() {
        for x in 0..WIDTH {
            let offset = (y - rows.start) * WIDTH + x;
            let mask = effects.mask(
                control,
                x,
                y,
                objects.window[offset],
                vertical_windows,
                horizontal_windows,
            );
            let mut top = backdrop;
            let mut below = None;
            let mut insert = |pixel: Pixel| {
                if pixel.order() < top.order() {
                    below = Some(top);
                    top = pixel;
                } else if below.is_none_or(|second: Pixel| pixel.order() < second.order()) {
                    below = Some(pixel);
                }
            };
            for &bg in &layers {
                // Validate enabled background fetches even when a window hides them.
                // Mosaic samples before scroll or affine mapping. Captured vertical
                // phase comes from a counter; horizontal blocks remain screen-aligned.
                // Window selection still uses the actual output coordinate.
                let (sample_x, sample_y) = if backgrounds[bg].control & 0x40 != 0 {
                    let counter =
                        vertical_mosaic.map_or(y % mosaic_height, |v| usize::from(v.background));
                    (x - x % mosaic_width, y - counter)
                } else {
                    (x, y)
                };
                let color = if mode == 0 || (mode == 1 && bg < 2) {
                    text_pixel(backgrounds[bg], vram, palette, sample_x, sample_y)?
                } else {
                    affine::pixel(
                        backgrounds[bg],
                        vram,
                        palette,
                        sample_x,
                        sample_y,
                        control,
                        internal_affine,
                    )?
                };
                if let Some(color) = color.filter(|_| mask & (1 << bg) != 0) {
                    insert(Pixel {
                        color,
                        layer: bg as u8,
                        priority: (backgrounds[bg].control & 3) as u8,
                        semi_transparent: false,
                    });
                }
            }
            if let Some(object) = objects.pixels[offset].filter(|_| mask & 0x10 != 0) {
                insert(object);
            }
            output[offset] = rgb555_to_rgb888(effects.apply(top, below, mask & 0x20 != 0));
        }
    }
    if green_swap {
        for pair in output.chunks_exact_mut(2) {
            let green_difference = (pair[0] ^ pair[1]) & 0x0000_ff00;
            pair[0] ^= green_difference;
            pair[1] ^= green_difference;
        }
    }
    Ok(output)
}

fn halfword(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// Translate a BG VRAM offset through the bus's 128 KiB mirror window.
fn vram_index(offset: usize) -> usize {
    let offset = offset & 0x1ffff;
    if offset >= 0x18000 {
        offset - 0x8000
    } else {
        offset
    }
}

fn text_pixel(
    bg: Background,
    vram: &[u8],
    palette: &[u8],
    x: usize,
    y: usize,
) -> Result<Option<u16>, VideoError> {
    let control = usize::from(bg.control);
    let width = if control & 0x4000 != 0 { 512 } else { 256 };
    let height = if control & 0x8000 != 0 { 512 } else { 256 };
    // Text backgrounds always wrap; the affine overflow bit has no effect.
    let x = (x + usize::from(bg.x)) % width;
    let y = (y + usize::from(bg.y)) % height;
    let block = x / 256 + (y / 256) * (width / 256);
    let map = ((control >> 8) & 31) * 0x800 + block * 0x800 + ((y / 8 % 32) * 32 + x / 8 % 32) * 2;
    if map >= 0x20000 {
        return Err(VideoError::UnsupportedMapAddress(map));
    }
    let entry = usize::from(halfword(vram, vram_index(map)));
    let tx = (x & 7) ^ if entry & 0x400 != 0 { 7 } else { 0 };
    let ty = (y & 7) ^ if entry & 0x800 != 0 { 7 } else { 0 };
    let eight_bit = control & 0x80 != 0;
    let pixel = ty * 8 + tx;
    let address = ((control >> 2) & 3) * 0x4000
        + (entry & 0x3ff) * if eight_bit { 64 } else { 32 }
        + if eight_bit { pixel } else { pixel / 2 };
    if address >= 0x20000 {
        return Err(VideoError::UnsupportedTileAddress(address));
    }
    let address = vram_index(address);
    let index = if eight_bit {
        usize::from(vram[address])
    } else {
        usize::from((vram[address] >> ((pixel & 1) * 4)) & 15)
    };
    if index == 0 {
        return Ok(None);
    }
    let bank = if eight_bit { 0 } else { (entry >> 12) * 16 };
    Ok(Some(halfword(palette, (bank + index) * 2)))
}

pub struct Framebuffer {
    pixels: Vec<u32>,
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self {
            pixels: vec![0; WIDTH * HEIGHT],
        }
    }
}

impl Framebuffer {
    /// Packed 0x00RRGGBB pixels in row-major order.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn clear(&mut self, color: u16) {
        self.pixels.fill(rgb555_to_rgb888(color));
    }

    /// Write a GBA RGB555 color. Out-of-bounds coordinates return false.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u16) -> bool {
        if x >= WIDTH || y >= HEIGHT {
            return false;
        }
        self.pixels[y * WIDTH + x] = rgb555_to_rgb888(color);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_gba_channel_order_and_ignores_unused_bit() {
        for (color, expected) in [
            (0, 0),
            (0x001f, 0x00ff_0000),
            (0x03e0, 0x0000_ff00),
            (0x7c00, 0x0000_00ff),
            (0x7fff, 0x00ff_ffff),
            (0xffff, 0x00ff_ffff),
            (0x8000, 0),
            (0x4210, 0x0084_8484),
        ] {
            assert_eq!(rgb555_to_rgb888(color), expected);
        }
    }

    #[test]
    fn starts_with_a_black_gba_sized_buffer() {
        let frame = Framebuffer::default();
        assert_eq!(frame.pixels().len(), 240 * 160);
        assert!(frame.pixels().iter().all(|pixel| *pixel == 0));
    }

    #[test]
    fn writes_row_major_pixels_including_last_pixel() {
        let mut frame = Framebuffer::default();
        assert!(frame.set_pixel(1, 2, 0x001f));
        assert!(frame.set_pixel(WIDTH - 1, HEIGHT - 1, 0x03e0));
        assert_eq!(frame.pixels()[2 * WIDTH + 1], 0x00ff_0000);
        assert_eq!(frame.pixels()[WIDTH * HEIGHT - 1], 0x0000_ff00);
        assert_eq!(frame.pixels()[0], 0);
    }

    #[test]
    fn clips_invalid_coordinates_without_changing_pixels() {
        let mut frame = Framebuffer::default();
        for (x, y) in [(WIDTH, 0), (0, HEIGHT), (usize::MAX, usize::MAX)] {
            assert!(!frame.set_pixel(x, y, 0x7fff));
        }
        assert!(frame.pixels().iter().all(|pixel| *pixel == 0));
    }

    #[test]
    fn clear_replaces_every_pixel() {
        let mut frame = Framebuffer::default();
        frame.set_pixel(0, 0, 0x001f);
        frame.clear(0x7c00);
        assert!(frame.pixels().iter().all(|pixel| *pixel == 0x0000_00ff));
    }
}
