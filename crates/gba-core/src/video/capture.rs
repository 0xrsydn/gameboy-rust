//! Optional scanline frame capture. Rows sample at HBlank entry; publication
//! happens at VBlank entry. This is not a pixel fetch pipeline.

use super::{Framebuffer, VideoError, HEIGHT, WIDTH};

pub(crate) struct Capture {
    drawing: Vec<u32>,
    completed: Vec<u32>,
    rows: usize,
    drawing_error: Option<VideoError>,
    completed_error: Option<VideoError>,
    vblank: Option<u64>,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            drawing: vec![0; WIDTH * HEIGHT],
            completed: vec![0; WIDTH * HEIGHT],
            rows: 0,
            drawing_error: None,
            completed_error: None,
            vblank: None,
        }
    }
}

impl Capture {
    /// Starting capture in the middle of a frame must not publish missing rows.
    pub(crate) fn wants_row(&self, row: usize) -> bool {
        row == 0 || (self.rows != 0 && row == self.rows)
    }

    pub(crate) fn record(&mut self, row: usize, pixels: Result<Vec<u32>, VideoError>) {
        if row == 0 {
            self.rows = 0;
            self.drawing_error = None;
        }
        debug_assert_eq!(self.rows, row);
        match pixels {
            Ok(pixels) => self.drawing[row * WIDTH..(row + 1) * WIDTH].copy_from_slice(&pixels),
            Err(error) => {
                self.drawing_error.get_or_insert(error);
            }
        }
        self.rows += 1;
    }

    pub(crate) fn publish(&mut self, vblank: u64) {
        if self.rows != HEIGHT {
            return;
        }
        self.completed_error = self.drawing_error.take();
        if self.completed_error.is_none() {
            std::mem::swap(&mut self.drawing, &mut self.completed);
        }
        self.vblank = Some(vblank);
        self.rows = 0;
    }

    pub(crate) fn vblank(&self) -> Option<u64> {
        self.vblank
    }

    /// Errors leave the caller's last displayed image unchanged.
    pub(crate) fn present(&self, output: &mut Framebuffer) -> Result<bool, VideoError> {
        if self.vblank.is_none() {
            return Ok(false);
        }
        if let Some(error) = &self.completed_error {
            return Err(error.clone());
        }
        output.pixels.copy_from_slice(&self.completed);
        Ok(true)
    }
}
