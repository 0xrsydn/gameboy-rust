//! GBA display clock and status, independent of pixel rendering.
//! Events use supplied clock cycles. Device updates occur at instruction, DMA,
//! or HALT-idle boundaries; display bus contention is not modeled. The request delay
//! lives in `io`. Forced blank does not stop this clock.

use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

pub const CPU_HZ: u32 = 16_777_216;
pub const CYCLES_PER_LINE: u32 = 1232;
pub const HBLANK_START: u32 = 1006;
pub const VISIBLE_LINES: u32 = 160;
pub const LINES_PER_FRAME: u32 = 228;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_LINE * LINES_PER_FRAME;
pub const VBLANK_START: u32 = CYCLES_PER_LINE * VISIBLE_LINES;

/// Display position after the latest clock advance. Event counters wrap at u64::MAX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayPosition {
    pub scanline: u16,
    pub line_cycle: u16,
    /// Number of completed 228-line frames (increments on entry to line zero).
    pub frames: u64,
    /// Number of entries to line 160, independent of local interrupt enables.
    pub vblanks: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Display {
    phase: u32,
    control: u16,
    frames: u64,
    vblanks: u64,
}

impl Display {
    pub(crate) fn position(&self) -> DisplayPosition {
        DisplayPosition {
            scanline: (self.phase / CYCLES_PER_LINE) as u16,
            line_cycle: (self.phase % CYCLES_PER_LINE) as u16,
            frames: self.frames,
            vblanks: self.vblanks,
        }
    }

    pub(crate) fn status(&self) -> u16 {
        let position = self.position();
        // Line 227 is in the non-visible region, but the VBlank flag is clear.
        let vblank = (160..227).contains(&position.scanline);
        let hblank = u32::from(position.line_cycle) >= HBLANK_START;
        let matched = position.scanline == self.control >> 8;
        self.control | u16::from(vblank) | (u16::from(hblank) << 1) | (u16::from(matched) << 2)
    }

    /// Write a DISPSTAT byte. Status bits and unused GBA bits are read-only.
    /// Changing the comparator can generate a rising match event immediately.
    /// Enabling an IRQ while its status is already set does not synthesize an edge.
    pub(crate) fn write_status(&mut self, high: bool, value: u8) -> u16 {
        let previously_matched = self.status() & 4 != 0;
        let shift = u32::from(high) * 8;
        self.control = ((self.control & !(0xff << shift)) | (u16::from(value) << shift)) & 0xff38;
        if !previously_matched && self.status() & 4 != 0 && self.control & 0x20 != 0 {
            4
        } else {
            0
        }
    }

    /// Next HBlank entry or scanline start. Both are strictly in the future.
    pub(crate) fn next_event_cycles(&self) -> u32 {
        let cycle = self.phase % CYCLES_PER_LINE;
        if cycle < HBLANK_START {
            HBLANK_START - cycle
        } else {
            CYCLES_PER_LINE - cycle
        }
    }

    /// Display DMA requests are independent of DISPSTAT interrupt enables.
    /// Unlike HBlank IRQs, HBlank DMA requests occur only on visible lines.
    pub(crate) fn dma_events(&self, cycles: u32) -> (bool, bool) {
        let vblank = crossings(self.phase, cycles, CYCLES_PER_FRAME, VBLANK_START) != 0;
        let visible_hblanks = |time: u64| {
            let frame = u64::from(CYCLES_PER_FRAME);
            let line = u64::from(CYCLES_PER_LINE);
            let visible = u64::from(VISIBLE_LINES);
            time / frame * visible
                + ((time % frame + line - u64::from(HBLANK_START)) / line).min(visible)
        };
        let begin = u64::from(self.phase);
        let hblank = visible_hblanks(begin + u64::from(cycles)) != visible_hblanks(begin);
        (vblank, hblank)
    }

    /// Advance inside one line without reaching its HBlank or the next line start.
    /// No status flag changes and no event counter moves in such an interval.
    pub(crate) fn advance_within_line(&mut self, cycles: u32) {
        let line_cycle = self.phase % CYCLES_PER_LINE;
        debug_assert!(line_cycle + cycles < CYCLES_PER_LINE);
        debug_assert!(line_cycle >= HBLANK_START || line_cycle + cycles < HBLANK_START);
        self.phase += cycles;
    }

    /// Return IRQ request bits crossed during the interval (old time, new time].
    /// Periodic arithmetic handles large batches without skipping events or
    /// looping per cycle/scanline. Status derives from the final clock position.
    pub(crate) fn advance(&mut self, cycles: u32) -> u16 {
        let vblanks = crossings(self.phase, cycles, CYCLES_PER_FRAME, VBLANK_START);
        let frames = crossings(self.phase, cycles, CYCLES_PER_FRAME, 0);
        let mut requests = 0;
        if self.control & 8 != 0 && vblanks != 0 {
            requests |= 1;
        }
        // HBlank status and IRQ events occur on all 228 lines, including VBlank.
        if self.control & 0x10 != 0
            && crossings(self.phase, cycles, CYCLES_PER_LINE, HBLANK_START) != 0
        {
            requests |= 2;
        }
        let compare = u32::from(self.control >> 8);
        if self.control & 0x20 != 0
            && compare < LINES_PER_FRAME
            && crossings(
                self.phase,
                cycles,
                CYCLES_PER_FRAME,
                compare * CYCLES_PER_LINE,
            ) != 0
        {
            requests |= 4;
        }
        self.phase =
            ((u64::from(self.phase) + u64::from(cycles)) % u64::from(CYCLES_PER_FRAME)) as u32;
        self.vblanks = self.vblanks.wrapping_add(vblanks);
        self.frames = self.frames.wrapping_add(frames);
        requests
    }
}

fn crossings(start: u32, elapsed: u32, period: u32, offset: u32) -> u64 {
    let shift = u64::from(period - offset);
    let begin = u64::from(start) + shift;
    (begin + u64::from(elapsed)) / u64::from(period) - begin / u64::from(period)
}

impl Snapshot for Display {
    fn save(&self, writer: &mut Writer) {
        let Self {
            phase,
            control,
            frames,
            vblanks,
        } = self;
        writer.tag(b"DISP");
        writer.u32(*phase);
        writer.u16(*control);
        writer.u64(*frames);
        writer.u64(*vblanks);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            phase,
            control,
            frames,
            vblanks,
        } = self;
        reader.tag(b"DISP")?;
        *phase = reader.u32()?;
        *control = reader.u16()?;
        *frames = reader.u64()?;
        *vblanks = reader.u64()?;
        if *phase >= CYCLES_PER_FRAME {
            return Err(SnapshotError::Corrupt("display phase"));
        }
        if *control & !0xff38 != 0 {
            return Err(SnapshotError::Corrupt("display control"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encode(display: &Display) -> Vec<u8> {
        let mut writer = Writer::new();
        display.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn round_trip_and_reject_every_strict_prefix() {
        let source = Display {
            phase: CYCLES_PER_LINE * 137 + 1009,
            control: 0xab38,
            frames: 0x1234_5678,
            vblanks: 0x8765_4321,
        };
        let bytes = encode(&source);
        let mut restored = Display::default();
        let mut reader = Reader::new(&bytes);
        restored.load(&mut reader).unwrap();
        reader.finish().unwrap();
        assert_eq!(restored, source);

        for end in 0..bytes.len() {
            let mut truncated = Reader::new(&bytes[..end]);
            assert!(
                Display::default().load(&mut truncated).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn out_of_range_phase_is_corrupt() {
        let mut bytes = encode(&Display::default());
        bytes[4..8].copy_from_slice(&CYCLES_PER_FRAME.to_le_bytes());
        let result = Display::default().load(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(SnapshotError::Corrupt(_))));
    }
}
