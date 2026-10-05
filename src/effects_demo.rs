//! Original CPU-driven window and color-effects scene, based on the tile demo.
//! The CPU writes window bounds and effect registers during each VBlank.

use crate::{
    graphics_demo::{GraphicsDemo, GraphicsError},
    input::Buttons,
    machine::Machine,
    memory::MemoryError,
    video::Framebuffer,
};

/// Completed updates and nine-bit horizontal/vertical scroll, as three words.
pub const EFFECTS_STATE: u32 = crate::tile_demo::TILE_STATE;

pub struct EffectsDemo {
    runner: GraphicsDemo,
}

impl EffectsDemo {
    pub fn new() -> Result<Self, MemoryError> {
        Ok(Self {
            runner: GraphicsDemo::with_program(crate::tile_demo::effects_program())?,
        })
    }

    pub fn machine(&self) -> &Machine {
        self.runner.machine()
    }

    /// Arrows scroll and move WIN0. Z selects alpha; X selects darkening.
    /// Otherwise the area outside WIN0 is brightened. Enter resets scrolling.
    /// Q/W retain the tile demo's sprite rotation and zoom controls.
    pub fn frame(
        &mut self,
        buttons: Buttons,
        output: &mut Framebuffer,
    ) -> Result<usize, GraphicsError> {
        self.runner.frame_with_control(buttons, output, 0x3340)
    }
}
