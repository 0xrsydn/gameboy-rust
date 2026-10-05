//! Original Mode 4/5 page-flipping demos. ARM code copies both images through
//! BIOS services, samples input, and switches the displayed page during VBlank.

use crate::{
    affine_demo,
    graphics_demo::{GraphicsDemo, GraphicsError},
};
use gba_core::{input::Buttons, machine::Machine, memory::MemoryError, video::Framebuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BitmapMode {
    Mode4 = 4,
    Mode5 = 5,
}

/// Debug words: completed updates, signed pan X, signed pan Y.
pub const BITMAP_STATE: u32 = 0x0200_0000;

pub struct BitmapDemo {
    runner: GraphicsDemo,
    mode: BitmapMode,
}
impl BitmapDemo {
    pub fn new(mode: BitmapMode) -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(affine_demo::bitmap_program(mode as u16))?,
            mode,
        })
    }
    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner
            .frame_with_control(buttons, output, 0x400 | self.mode as u16)
    }
}

pub(crate) fn assets(mode: u16) -> Vec<u8> {
    let mut palette = [0_u16; 256];
    palette[0] = 0x4000;
    for (i, color) in palette.iter_mut().enumerate().skip(1) {
        *color = ((i & 31) | (((i >> 3) & 31) << 5) | (((i * 3) & 31) << 10)) as u16;
    }
    let mut data: Vec<u8> = palette.into_iter().flat_map(u16::to_le_bytes).collect();
    let mut vram = vec![0_u8; 0x14000];
    let (width, height) = if mode == 4 { (240, 160) } else { (160, 128) };
    for page in 0..2 {
        for y in 0..height {
            for x in 0..width {
                let offset = y * width + x;
                if mode == 4 {
                    vram[page * 0xa000 + offset] = ((x / 8 + 3 * (y / 8) + page * 64) & 255) as u8;
                } else {
                    let color = (((x / 5 + page * 11) & 31)
                        | (((y / 4 + page * 7) & 31) << 5)
                        | (((x / 8 + y / 8 + page * 5) & 31) << 10))
                        as u16;
                    let address = page * 0xa000 + offset * 2;
                    vram[address..address + 2].copy_from_slice(&color.to_le_bytes());
                }
            }
        }
    }
    data.extend(vram);
    data
}
