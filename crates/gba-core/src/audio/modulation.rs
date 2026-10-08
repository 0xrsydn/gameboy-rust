//! Shared nominal PSG length, envelope, activity, and logical DAC gate.

use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Modulation {
    pub(super) active: bool,
    pub(super) envelope: u8,
    pub(super) length_enabled: bool,
    pub(super) length: u8,
    pub(super) volume: u8,
    envelope_ticks: u8,
    envelope_running: bool,
}

impl Modulation {
    pub(super) fn write_length(&mut self, value: u8) {
        self.length = 64 - (value & 63);
    }

    fn dac_enabled(&self) -> bool {
        self.envelope & 0xf8 != 0
    }

    fn envelope_can_run(&self) -> bool {
        self.envelope & 7 != 0
            && if self.envelope & 8 != 0 {
                self.volume < 15
            } else {
                self.volume > 0
            }
    }

    pub(super) fn write_envelope(&mut self, value: u8) {
        let was_running = self.envelope_running;
        self.envelope = value;
        if !self.dac_enabled() {
            self.active = false;
        }
        self.envelope_running = self.envelope_can_run();
        if !was_running && self.envelope_running {
            self.envelope_ticks = value & 7;
        }
    }

    /// Apply length-enable edge rules and restart modulation when bit 7 is set.
    /// Return the trigger strobe even with the logical DAC gate disabled.
    pub(super) fn write_control(&mut self, value: u8, next_step: u8) -> bool {
        let extra_length_clock = next_step & 1 != 0;
        if !self.length_enabled && value & 0x40 != 0 && extra_length_clock {
            self.clock_length();
        }
        self.length_enabled = value & 0x40 != 0;
        if value & 0x80 == 0 {
            return false;
        }
        self.active = self.dac_enabled();
        if self.length == 0 {
            self.length = 64;
            if self.length_enabled && extra_length_clock {
                self.length -= 1;
            }
        }
        self.volume = self.envelope >> 4;
        self.envelope_ticks = if self.envelope & 7 == 0 {
            8
        } else {
            self.envelope & 7
        };
        self.envelope_running = self.envelope_can_run();
        true
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
        if step == 7 && self.active && self.envelope_running {
            self.envelope_ticks -= 1;
            if self.envelope_ticks == 0 {
                if self.envelope & 8 != 0 {
                    self.volume += 1;
                } else {
                    self.volume -= 1;
                }
                self.envelope_ticks = self.envelope & 7;
                self.envelope_running = self.envelope_can_run();
            }
        }
    }
}

impl Snapshot for Modulation {
    fn save(&self, writer: &mut Writer) {
        let Self {
            active,
            envelope,
            length_enabled,
            length,
            volume,
            envelope_ticks,
            envelope_running,
        } = self;
        writer.tag(b"MOD0");
        writer.bool(*active);
        writer.u8(*envelope);
        writer.bool(*length_enabled);
        writer.u8(*length);
        writer.u8(*volume);
        writer.u8(*envelope_ticks);
        writer.bool(*envelope_running);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"MOD0")?;
        let Self {
            active,
            envelope,
            length_enabled,
            length,
            volume,
            envelope_ticks,
            envelope_running,
        } = self;
        *active = reader.bool()?;
        *envelope = reader.u8()?;
        *length_enabled = reader.bool()?;
        *length = reader.u8()?;
        *volume = reader.u8()?;
        *envelope_ticks = reader.u8()?;
        *envelope_running = reader.bool()?;
        if *length > 64 || *volume > 15 || *envelope_ticks > 8 {
            return Err(SnapshotError::Corrupt("modulation range"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshot_round_trip_prefixes_and_invalid_boolean() {
        let source = Modulation {
            active: true,
            envelope: 0xf3,
            length_enabled: true,
            length: 29,
            volume: 7,
            envelope_ticks: 4,
            envelope_running: true,
        };
        let mut writer = Writer::new();
        source.save(&mut writer);
        let bytes = writer.finish().unwrap();

        let mut loaded = Modulation::default();
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded, source);
        for end in 0..bytes.len() {
            assert!(loaded.load(&mut Reader::new(&bytes[..end])).is_err());
        }

        let mut corrupt = bytes;
        corrupt[4] = 2;
        assert_eq!(
            loaded.load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("boolean"))
        );
    }
}
