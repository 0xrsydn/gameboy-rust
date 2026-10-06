//! ROM window execution without window-library or host-clock dependencies.
//! No demo-specific display register, RAM mailbox, or startup condition is used.

use std::{
    error::Error,
    io::{self, Write},
};

use gba_core::{bios, input::Buttons, machine::Machine, memory::MemoryError, video::Framebuffer};

use super::{write_state, Stats};

/// Yield to the host after this many steps, even before a frame completes.
const SLICE_STEPS: usize = 4096;
/// Guard progress between VBlank events; retained across slices and STOP waits.
// A frame takes 280,896 cycles. Leave room for one-cycle instructions in RAM.
const FRAME_STEPS: usize = 400_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Update {
    Running,
    Frame,
    Stopped,
}

pub struct Session {
    machine: Machine,
    frame: Framebuffer,
    stats: Stats,
    frames: u64,
    frame_steps: usize,
    vblank: u64,
}

impl Session {
    pub fn new(bytes: Vec<u8>) -> Result<Self, MemoryError> {
        let mut machine = bios::boot(bytes)?;
        machine.memory_mut().set_scanline_rendering(true);
        Ok(Self {
            machine,
            frame: Framebuffer::default(),
            stats: Stats::default(),
            frames: 0,
            frame_steps: 0,
            vblank: 0,
        })
    }

    pub fn set_cartridge_hardware(&mut self, hardware: gba_core::cartridge::CartridgeHardware) {
        self.machine.memory_mut().set_cartridge_hardware(hardware);
    }

    pub fn frame(&self) -> &Framebuffer {
        &self.frame
    }
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Sample input, then advance at most one bounded slice or one VBlank event.
    /// STOP preserves clocks and the last image while allowing later input to wake.
    /// Diagnostics preserve the last presented image and earlier successful steps.
    pub fn update(&mut self, buttons: Buttons) -> Result<Update, Box<dyn Error>> {
        self.update_bounded(buttons, SLICE_STEPS, FRAME_STEPS)
    }

    fn update_bounded(
        &mut self,
        buttons: Buttons,
        slice: usize,
        frame_limit: usize,
    ) -> Result<Update, Box<dyn Error>> {
        self.machine.memory_mut().set_buttons(buttons);
        for _ in 0..slice {
            if self.machine.stopped() {
                return Ok(Update::Stopped);
            }
            if self.frame_steps >= frame_limit {
                return Err(io::Error::other(format!(
                    "ROM exceeded {frame_limit} machine steps without VBlank"
                ))
                .into());
            }
            let kind = self.machine.step()?;
            self.stats.record(kind);
            self.frame_steps += 1;
            let vblank = self.machine.memory().display_position().vblanks;
            if vblank != self.vblank {
                if !self.machine.memory().present_frame(&mut self.frame)? {
                    return Err(io::Error::other(
                        "ROM reached VBlank without a complete captured frame",
                    )
                    .into());
                }
                self.vblank = vblank;
                self.frame_steps = 0;
                self.frames += 1;
                return Ok(Update::Frame);
            }
        }
        if self.machine.stopped() {
            Ok(Update::Stopped)
        } else {
            Ok(Update::Running)
        }
    }

    pub fn report(&self, writer: &mut impl Write, reason: &str) -> io::Result<()> {
        writeln!(writer, "Result: {reason}")?;
        writeln!(writer, "Captured ROM frames: {}", self.frames)?;
        write_state(writer, &self.machine, &self.stats)?;
        writer.flush()
    }
}

#[cfg(test)]
mod tests;
