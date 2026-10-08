//! Nominal single-bank wave playback. RAM rotates in high-nibble-first order.
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Wave {
    control: u8,
    volume: u8,
    frequency: u16,
    length: u16,
    length_enabled: bool,
    pub(super) active: bool,
    remaining: u32,
    sample: Option<u8>,
}

impl Wave {
    pub(super) fn bank(&self) -> usize {
        usize::from((self.control >> 6) & 1)
    }

    pub(super) fn unsupported(&self, offset: u32, value: u8) -> Option<&'static str> {
        if (offset == 5 && value & 0x80 != 0 && self.control & 0xa0 == 0xa0)
            || (offset == 0 && self.active && value & 0xa0 == 0xa0)
        {
            return Some("64-sample wave playback");
        }
        if offset == 0 && self.active && value & 0x80 != 0 && (value ^ self.control) & 0x40 != 0 {
            return Some("wave bank change during playback");
        }
        None
    }

    pub(super) fn read(&self, offset: u32) -> u8 {
        match offset {
            0 => self.control,
            3 => self.volume,
            5 => u8::from(self.length_enabled) << 6,
            _ => 0,
        }
    }

    fn period(&self) -> u32 {
        8 * (2048 - u32::from(self.frequency))
    }

    pub(super) fn write(&mut self, offset: u32, value: u8, next_step: u8) {
        match offset {
            0 => {
                self.control = value & 0xe0;
                if value & 0x80 == 0 {
                    self.active = false;
                }
            }
            2 => self.length = 256 - u16::from(value),
            3 => self.volume = value & 0xe0,
            4 => self.frequency = (self.frequency & 0x700) | u16::from(value),
            5 => {
                self.frequency = (self.frequency & 255) | (u16::from(value & 7) << 8);
                let extra_clock = next_step & 1 != 0;
                if !self.length_enabled && value & 0x40 != 0 && extra_clock {
                    self.clock_length();
                }
                self.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.active = self.control & 0x80 != 0;
                    if self.length == 0 {
                        self.length = 256 - u16::from(self.length_enabled && extra_clock);
                    }
                    self.remaining = self.period();
                    // Nominal startup policy: silence until the first full sample period.
                    // Retrigger cannot restore RAM that playback has already rotated.
                    self.sample = None;
                }
            }
            _ => {}
        }
    }

    fn clock_length(&mut self) {
        if self.length != 0 {
            self.length -= 1;
            if self.length == 0 {
                self.active = false;
            }
        }
    }

    pub(super) fn clock(&mut self, step: u8) {
        if step & 1 == 0 && self.length_enabled {
            self.clock_length();
        }
    }

    pub(super) fn advance(&mut self, cycles: u32, banks: &mut [[u8; 16]; 2]) {
        if !self.active || cycles == 0 {
            return;
        }
        if cycles < self.remaining {
            self.remaining -= cycles;
            return;
        }
        let rest = cycles - self.remaining;
        let edges = 1 + rest / self.period();
        self.remaining = self.period() - rest % self.period();
        let data = &mut banks[self.bank()];
        let old = *data;
        let digit = |index: usize| {
            let byte = old[(index % 32) / 2];
            (byte >> if index & 1 == 0 { 4 } else { 0 }) & 15
        };
        self.sample = Some(digit(((edges - 1) % 32) as usize));
        let rotation = (edges % 32) as usize;
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (digit(index * 2 + rotation) << 4) | digit(index * 2 + rotation + 1);
        }
    }

    /// Quarter-units of the pulse/noise amplitude. Keep fractional wave volume
    /// until after routing, master volume, and PSG ratio have been combined.
    pub(super) fn sample_quarters(&self) -> i16 {
        if !self.active {
            return 0;
        }
        let Some(sample) = self.sample else { return 0 };
        let gain = if self.volume & 0x80 != 0 {
            3
        } else {
            [0, 4, 2, 1][usize::from(self.volume >> 5)]
        };
        (i16::from(sample) - 8) * 2 * gain
    }
}

impl Snapshot for Wave {
    fn save(&self, writer: &mut Writer) {
        let Self {
            control,
            volume,
            frequency,
            length,
            length_enabled,
            active,
            remaining,
            sample,
        } = self;
        writer.tag(b"WAV0");
        writer.u8(*control);
        writer.u8(*volume);
        writer.u16(*frequency);
        writer.u16(*length);
        writer.bool(*length_enabled);
        writer.bool(*active);
        writer.u32(*remaining);
        writer.option(sample.as_ref(), |sample, writer| writer.u8(*sample));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"WAV0")?;
        let Self {
            control,
            volume,
            frequency,
            length,
            length_enabled,
            active,
            remaining,
            sample,
        } = self;
        *control = reader.u8()?;
        *volume = reader.u8()?;
        *frequency = reader.u16()?;
        *length = reader.u16()?;
        *length_enabled = reader.bool()?;
        *active = reader.bool()?;
        *remaining = reader.u32()?;
        *sample = reader.option(|reader| reader.u8())?;
        if *control & !0xe0 != 0
            || *volume & !0xe0 != 0
            || *frequency > 2047
            || *length > 256
            || *remaining > 16384
            || matches!(*sample, Some(value) if value > 15)
        {
            return Err(SnapshotError::Corrupt("wave range"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshot_round_trip_prefixes_and_invalid_option_tag() {
        let mut source = Wave::default();
        source.write(0, 0xa0, 0);
        source.write(2, 55, 0);
        source.write(3, 0xe0, 0);
        source.write(4, 0x7b, 0);
        source.write(5, 0xc1, 0);
        let mut banks = [[0x12; 16]; 2];
        source.advance(source.remaining, &mut banks);

        let mut writer = Writer::new();
        source.save(&mut writer);
        let bytes = writer.finish().unwrap();
        let mut loaded = Wave::default();
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded, source);
        for end in 0..bytes.len() {
            assert!(loaded.load(&mut Reader::new(&bytes[..end])).is_err());
        }

        let mut corrupt = bytes;
        corrupt[16] = 2;
        assert_eq!(
            loaded.load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("boolean"))
        );
    }
}

#[cfg(test)]
mod tests;
