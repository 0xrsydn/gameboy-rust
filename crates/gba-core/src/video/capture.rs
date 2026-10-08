//! Optional scanline frame capture. Rows sample at HBlank entry; publication
//! happens at VBlank entry. This is not a pixel fetch pipeline.

use super::{Framebuffer, VideoError, HEIGHT, WIDTH};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

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

impl Snapshot for Capture {
    fn save(&self, writer: &mut Writer) {
        let Self {
            drawing,
            completed,
            rows,
            drawing_error,
            completed_error,
            vblank,
        } = self;
        writer.tag(b"CAPT");
        let pixels = WIDTH * HEIGHT;
        if drawing.len() != pixels || completed.len() != pixels {
            writer.fail(SnapshotError::Unsupported("invalid capture buffer size"));
        }
        writer.u64(drawing.len() as u64);
        for pixel in drawing {
            writer.u32(*pixel);
        }
        writer.u64(completed.len() as u64);
        for pixel in completed {
            writer.u32(*pixel);
        }
        if *rows > HEIGHT {
            writer.fail(SnapshotError::Unsupported("invalid capture row count"));
        }
        writer.u64(*rows as u64);
        save_video_error(drawing_error, writer);
        save_video_error(completed_error, writer);
        writer.option(vblank.as_ref(), |value, writer| writer.u64(*value));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            drawing,
            completed,
            rows,
            drawing_error,
            completed_error,
            vblank,
        } = self;
        reader.tag(b"CAPT")?;
        let pixels = WIDTH * HEIGHT;
        if drawing.len() != pixels || completed.len() != pixels {
            return Err(SnapshotError::Corrupt("capture buffer size"));
        }
        const MIN_PAYLOAD: usize = 8 + WIDTH * HEIGHT * 4 + 8 + WIDTH * HEIGHT * 4 + 8 + 1 + 1 + 1;
        if reader.remaining() < MIN_PAYLOAD {
            return Err(SnapshotError::Truncated);
        }
        let drawing_len = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("capture buffer size"))?;
        if drawing_len != pixels {
            return Err(SnapshotError::Corrupt("capture buffer size"));
        }
        for pixel in drawing {
            *pixel = reader.u32()?;
        }
        let completed_len = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("capture buffer size"))?;
        if completed_len != pixels {
            return Err(SnapshotError::Corrupt("capture buffer size"));
        }
        for pixel in completed {
            *pixel = reader.u32()?;
        }
        let loaded_rows = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("capture row count"))?;
        if loaded_rows > HEIGHT {
            return Err(SnapshotError::Corrupt("capture row count"));
        }
        *rows = loaded_rows;
        *drawing_error = load_video_error(reader)?;
        *completed_error = load_video_error(reader)?;
        *vblank = reader.option(|reader| reader.u64())?;
        Ok(())
    }
}

fn save_video_error(error: &Option<VideoError>, writer: &mut Writer) {
    writer.bool(error.is_some());
    if error.is_some() {
        writer.fail(SnapshotError::Unsupported("deferred video diagnostic"));
    }
}

fn load_video_error(reader: &mut Reader<'_>) -> Result<Option<VideoError>, SnapshotError> {
    if reader.bool()? {
        Err(SnapshotError::Corrupt("deferred video diagnostic"))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encoded(value: &Capture) -> Vec<u8> {
        let mut writer = Writer::new();
        value.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn capture_round_trips_buffers_and_rejects_every_strict_prefix() {
        let mut source = Capture::default();
        for (index, pixel) in source.drawing.iter_mut().enumerate() {
            *pixel = index as u32 ^ 0x1234_5678;
        }
        for (index, pixel) in source.completed.iter_mut().enumerate() {
            *pixel = index as u32 ^ 0x8765_4321;
        }
        source.rows = 37;
        source.vblank = Some(0x1234_5678_9abc_def0);
        let bytes = encoded(&source);
        let mut restored = Capture::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(encoded(&restored), bytes);
        let mut prefix_target = Capture::default();
        for end in 0..bytes.len() {
            assert!(
                prefix_target.load(&mut Reader::new(&bytes[..end])).is_err(),
                "prefix {end}"
            );
        }

        let mut invalid = bytes.clone();
        // The final nine bytes are the vblank option flag and its u64 payload.
        let option_flag = invalid.len() - 9;
        invalid[option_flag] = 2;
        assert_eq!(
            Capture::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("boolean"))
        );
        invalid[4] = 1;
        assert_eq!(
            Capture::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("capture buffer size"))
        );
    }

    #[test]
    fn deferred_capture_errors_cannot_be_saved() {
        let capture = Capture {
            drawing_error: Some(VideoError::UnsupportedMode(7)),
            ..Capture::default()
        };
        let mut writer = Writer::new();
        capture.save(&mut writer);
        assert_eq!(
            writer.finish(),
            Err(SnapshotError::Unsupported("deferred video diagnostic"))
        );
    }
}
