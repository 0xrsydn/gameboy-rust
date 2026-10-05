//! Original tile scene with CPU-controlled background and sprite mosaic sizes.

use crate::{
    graphics_demo::{GraphicsDemo, GraphicsError},
    input::Buttons,
    machine::Machine,
    memory::MemoryError,
    video::Framebuffer,
};

/// Completed updates and nine-bit horizontal/vertical scroll, as three words.
pub const MOSAIC_STATE: u32 = crate::tile_demo::TILE_STATE;

pub struct MosaicDemo {
    runner: GraphicsDemo,
}

impl MosaicDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(crate::tile_demo::mosaic_program())?,
        })
    }

    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }

    /// Block dimensions cycle from 1 to 16 pixels, changing every eight updates.
    /// Z bypasses BG mosaic; X bypasses OBJ mosaic. Q/W rotate/zoom the sprite.
    /// Arrows scroll; Enter resets scrolling. The host only supplies input.
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner.frame_with_control(buttons, output, 0x1340)
    }
}
