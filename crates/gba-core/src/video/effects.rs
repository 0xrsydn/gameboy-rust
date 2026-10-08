//! Window masks and five-bit color arithmetic for snapshots and row capture.
//! Snapshots use half-open, wrapping bounds. Row capture supplies vertical flags
//! and horizontal comparator history. Layer/effect selection remains row-sampled.

use super::Pixel;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Effects {
    pub horizontal: [u16; 2],
    pub vertical: [u16; 2],
    pub inside: u16,
    pub outside: u16,
    pub control: u16,
    pub alpha: u16,
    pub brightness: u16,
}

impl Effects {
    pub(super) fn mask(
        &self,
        display: u16,
        x: usize,
        y: usize,
        object: bool,
        vertical: Option<[bool; 2]>,
        horizontal: Option<&super::windows::horizontal::HorizontalWindows>,
    ) -> u16 {
        if display & 0xe000 == 0 {
            return 0x3f;
        }
        for window in 0..2 {
            if display & (0x2000 << window) != 0
                && horizontal.map_or_else(
                    || contains(self.horizontal[window], x),
                    |state| state.pixels[window][x],
                )
                && vertical
                    .map_or_else(|| contains(self.vertical[window], y), |flags| flags[window])
            {
                return (self.inside >> (window * 8)) & 0x3f;
            }
        }
        if display & 0x9000 == 0x9000 && object {
            (self.outside >> 8) & 0x3f
        } else {
            self.outside & 0x3f
        }
    }

    pub(super) fn apply(&self, top: Pixel, below: Option<Pixel>, enabled: bool) -> u16 {
        if !enabled {
            return top.color;
        }
        let mode = (self.control >> 6) & 3;
        let first = self.control & (1 << top.layer) != 0;
        if top.semi_transparent || (mode == 1 && first) {
            if let Some(second) = below.filter(|p| self.control & (0x100 << p.layer) != 0) {
                let a = (self.alpha & 31).min(16);
                let b = ((self.alpha >> 8) & 31).min(16);
                return channels(top.color, second.color, |x, y| {
                    ((x * a + y * b) >> 4).min(31)
                });
            }
        }
        // Semi-transparent OBJ falls back to a selected brightness effect only
        // when no eligible second target is directly below it.
        if first && mode >= 2 {
            let coefficient = (self.brightness & 31).min(16);
            return channels(top.color, 0, |x, _| {
                if mode == 2 {
                    x + (((31 - x) * coefficient) >> 4)
                } else {
                    x - ((x * coefficient) >> 4)
                }
            });
        }
        top.color
    }
}

fn contains(bounds: u16, coordinate: usize) -> bool {
    let start = usize::from(bounds >> 8);
    let end = usize::from(bounds & 255);
    if start > end {
        coordinate >= start || coordinate < end
    } else {
        (start..end).contains(&coordinate)
    }
}

fn channels(first: u16, second: u16, operation: impl Fn(u16, u16) -> u16) -> u16 {
    [0, 5, 10].into_iter().fold(0, |color, shift| {
        color | (operation((first >> shift) & 31, (second >> shift) & 31) << shift)
    })
}

impl Snapshot for Effects {
    fn save(&self, writer: &mut Writer) {
        let Self {
            horizontal,
            vertical,
            inside,
            outside,
            control,
            alpha,
            brightness,
        } = self;
        writer.tag(b"EFCT");
        for value in horizontal {
            writer.u16(*value);
        }
        for value in vertical {
            writer.u16(*value);
        }
        writer.u16(*inside);
        writer.u16(*outside);
        writer.u16(*control);
        writer.u16(*alpha);
        writer.u16(*brightness);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            horizontal,
            vertical,
            inside,
            outside,
            control,
            alpha,
            brightness,
        } = self;
        reader.tag(b"EFCT")?;
        for value in horizontal {
            *value = reader.u16()?;
        }
        for value in vertical {
            *value = reader.u16()?;
        }
        *inside = reader.u16()?;
        *outside = reader.u16()?;
        *control = reader.u16()?;
        *alpha = reader.u16()?;
        *brightness = reader.u16()?;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn effects_round_trip_and_reject_every_strict_prefix() {
        let source = Effects {
            horizontal: [0x1234, 0x5678],
            vertical: [0x9abc, 0xdef0],
            inside: 0x1357,
            outside: 0x2468,
            control: 0xaaaa,
            alpha: 0x5555,
            brightness: 0xdead,
        };
        let mut writer = Writer::new();
        source.save(&mut writer);
        let bytes = writer.finish().unwrap();
        let mut restored = Effects::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
        for end in 0..bytes.len() {
            assert!(Effects::default()
                .load(&mut Reader::new(&bytes[..end]))
                .is_err());
        }
        let mut invalid = bytes;
        invalid[0] ^= 1;
        assert_eq!(
            Effects::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("section tag"))
        );
    }
}
