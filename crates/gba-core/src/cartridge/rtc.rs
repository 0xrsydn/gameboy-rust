//! Edge-driven RTC command framing with latched reads and complete-payload writes.
use super::RtcDateTime;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rtc {
    control: u8,
    pub(super) calendar: RtcDateTime,
    selected: bool,
    clock: bool,
    sample: bool,
    pub(super) output: bool,
    phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Register {
    Control,
    DateTime,
    Time,
}

impl Register {
    fn len(self) -> u8 {
        match self {
            Self::Control => 1,
            Self::DateTime => 7,
            Self::Time => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Command {
        byte: u8,
        bits: u8,
    },
    Write {
        register: Register,
        data: [u8; 7],
        offset: u8,
        byte: u8,
        bits: u8,
    },
    Read {
        data: [u8; 7],
        len: u8,
        bit: u8,
    },
    Done,
}

impl Default for Rtc {
    fn default() -> Self {
        Self {
            control: 0x40,
            calendar: RtcDateTime::default(),
            selected: false,
            clock: true,
            sample: true,
            output: true,
            phase: Phase::Command { byte: 0, bits: 0 },
        }
    }
}

impl Rtc {
    pub(super) fn update(&mut self, pins: u8, direction: u8) -> Result<(), &'static str> {
        let selected = pins & 4 != 0;
        let clock = pins & 1 != 0;
        let data = pins & 2 != 0;
        if !selected {
            self.selected = false;
            self.clock = true;
            self.output = true;
            self.phase = Phase::Command { byte: 0, bits: 0 };
            return Ok(());
        }
        if !self.selected {
            if !clock {
                return Err("RTC select without high clock");
            }
            self.selected = true;
            self.clock = true;
            return Ok(());
        }
        if !clock {
            match self.phase {
                Phase::Command { .. } | Phase::Write { .. } => {
                    if direction & 2 == 0 {
                        return Err("RTC command/data write with SIO input");
                    }
                    self.sample = data;
                }
                Phase::Read { data, len, bit } if self.clock => {
                    if direction & 2 != 0 {
                        return Err("RTC read with SIO output");
                    }
                    self.output = data[usize::from(bit / 8)] & (1 << (bit % 8)) != 0;
                    self.phase = if bit + 1 == len * 8 {
                        Phase::Done
                    } else {
                        Phase::Read {
                            data,
                            len,
                            bit: bit + 1,
                        }
                    };
                }
                Phase::Done if self.clock => return Err("RTC clocks beyond command length"),
                _ => {}
            }
        } else if !self.clock && matches!(self.phase, Phase::Command { .. } | Phase::Write { .. }) {
            if direction & 2 == 0 || data != self.sample {
                return Err("RTC SIO change at sampling edge");
            }
            self.receive_bit()?;
        }
        self.clock = clock;
        Ok(())
    }

    fn receive_bit(&mut self) -> Result<(), &'static str> {
        self.phase = match self.phase {
            Phase::Command { byte, bits } => {
                let byte = byte | (u8::from(self.sample) << bits);
                if bits == 7 {
                    self.command(byte)?
                } else {
                    Phase::Command {
                        byte,
                        bits: bits + 1,
                    }
                }
            }
            Phase::Write {
                register,
                mut data,
                offset,
                byte,
                bits,
            } => {
                let byte = byte | (u8::from(self.sample) << bits);
                if bits != 7 {
                    Phase::Write {
                        register,
                        data,
                        offset,
                        byte,
                        bits: bits + 1,
                    }
                } else {
                    data[usize::from(offset)] = byte;
                    if offset + 1 == register.len() {
                        self.commit(register, data)?;
                        Phase::Done
                    } else {
                        Phase::Write {
                            register,
                            data,
                            offset: offset + 1,
                            byte: 0,
                            bits: 0,
                        }
                    }
                }
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn commit(&mut self, register: Register, data: [u8; 7]) -> Result<(), &'static str> {
        let hour24 = self.control & 0x40 != 0;
        match register {
            Register::Control => {
                if data[0] & 0x2a != 0 {
                    return Err("RTC interrupt/unknown control bits");
                }
                self.control = data[0] & 0x40;
            }
            Register::DateTime | Register::Time => {
                let data = if register == Register::Time {
                    let mut date = self.calendar.encode(hour24);
                    date[4..].copy_from_slice(&data[..3]);
                    date
                } else {
                    data
                };
                self.calendar =
                    RtcDateTime::decode(data, hour24).map_err(|_| "RTC invalid calendar data")?;
            }
        }
        Ok(())
    }

    fn command(&mut self, byte: u8) -> Result<Phase, &'static str> {
        // First four wire bits are 0,1,1,0. Convert to MSB-first command numbering.
        if byte & 15 != 6 {
            return Err("RTC command encoding");
        }
        let command = byte.reverse_bits();
        let register = match (command >> 1) & 7 {
            0 => {
                self.control = 0;
                self.calendar = RtcDateTime::default();
                return Ok(Phase::Done);
            }
            1 => Register::Control,
            2 => Register::DateTime,
            3 => Register::Time,
            6 => return Err("RTC force interrupt"),
            _ => return Err("RTC unused command"),
        };
        if command & 1 == 0 {
            Ok(Phase::Write {
                register,
                data: [0; 7],
                offset: 0,
                byte: 0,
                bits: 0,
            })
        } else {
            let mut data = self.calendar.encode(self.control & 0x40 != 0);
            match register {
                Register::Control => data[0] = self.control,
                Register::Time => {
                    data.copy_within(4..7, 0);
                }
                Register::DateTime => {}
            }
            Ok(Phase::Read {
                data,
                len: register.len(),
                bit: 0,
            })
        }
    }
}

impl Snapshot for Register {
    fn save(&self, writer: &mut Writer) {
        writer.tag(b"RTCR");
        writer.u8(match self {
            Self::Control => 0,
            Self::DateTime => 1,
            Self::Time => 2,
        });
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"RTCR")?;
        let decoded = match reader.u8()? {
            0 => Self::Control,
            1 => Self::DateTime,
            2 => Self::Time,
            _ => return Err(SnapshotError::Corrupt("RTC register")),
        };
        let _current = match self {
            Self::Control => 0,
            Self::DateTime => 1,
            Self::Time => 2,
        };
        *self = decoded;
        Ok(())
    }
}

impl Snapshot for Phase {
    fn save(&self, writer: &mut Writer) {
        writer.tag(b"RTPH");
        match self {
            Self::Command { byte, bits } => {
                writer.u8(0);
                writer.u8(*byte);
                writer.u8(*bits);
            }
            Self::Write {
                register,
                data,
                offset,
                byte,
                bits,
            } => {
                writer.u8(1);
                register.save(writer);
                writer.raw(data);
                writer.u8(*offset);
                writer.u8(*byte);
                writer.u8(*bits);
            }
            Self::Read { data, len, bit } => {
                writer.u8(2);
                writer.raw(data);
                writer.u8(*len);
                writer.u8(*bit);
            }
            Self::Done => writer.u8(3),
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"RTPH")?;
        let decoded = match reader.u8()? {
            0 => {
                let byte = reader.u8()?;
                let bits = reader.u8()?;
                if bits >= 8 {
                    return Err(SnapshotError::Corrupt("RTC command bit index"));
                }
                Self::Command { byte, bits }
            }
            1 => {
                let mut data = [0; 7];
                let mut register = Register::Control;
                register.load(reader)?;
                reader.raw_into(&mut data)?;
                let offset = reader.u8()?;
                let byte = reader.u8()?;
                let bits = reader.u8()?;
                if offset >= register.len() || bits >= 8 {
                    return Err(SnapshotError::Corrupt("RTC write index"));
                }
                Self::Write {
                    register,
                    data,
                    offset,
                    byte,
                    bits,
                }
            }
            2 => {
                let mut data = [0; 7];
                reader.raw_into(&mut data)?;
                let len = reader.u8()?;
                let bit = reader.u8()?;
                if !matches!(len, 1 | 3 | 7) || bit >= len * 8 {
                    return Err(SnapshotError::Corrupt("RTC read index"));
                }
                Self::Read { data, len, bit }
            }
            3 => Self::Done,
            _ => return Err(SnapshotError::Corrupt("RTC phase")),
        };
        let _current = match self {
            Self::Command { byte: _, bits: _ } => 0,
            Self::Write {
                register: _,
                data: _,
                offset: _,
                byte: _,
                bits: _,
            } => 1,
            Self::Read {
                data: _,
                len: _,
                bit: _,
            } => 2,
            Self::Done => 3,
        };
        *self = decoded;
        Ok(())
    }
}

impl Snapshot for Rtc {
    fn save(&self, writer: &mut Writer) {
        let Self {
            control,
            calendar,
            selected,
            clock,
            sample,
            output,
            phase,
        } = self;
        writer.tag(b"RTC0");
        writer.u8(*control);
        calendar.save(writer);
        writer.bool(*selected);
        writer.bool(*clock);
        writer.bool(*sample);
        writer.bool(*output);
        phase.save(writer);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            control,
            calendar,
            selected,
            clock,
            sample,
            output,
            phase,
        } = self;
        reader.tag(b"RTC0")?;
        let saved_control = reader.u8()?;
        if saved_control & !0x40 != 0 {
            return Err(SnapshotError::Corrupt("RTC control"));
        }
        calendar.load(reader)?;
        let saved_selected = reader.bool()?;
        let saved_clock = reader.bool()?;
        let saved_sample = reader.bool()?;
        let saved_output = reader.bool()?;
        phase.load(reader)?;
        *control = saved_control;
        *selected = saved_selected;
        *clock = saved_clock;
        *sample = saved_sample;
        *output = saved_output;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encoded<T: Snapshot>(value: &T) -> Vec<u8> {
        let mut writer = Writer::new();
        value.save(&mut writer);
        writer.finish().unwrap()
    }

    fn assert_prefixes<T: Snapshot + Copy>(value: T) {
        let bytes = encoded(&value);
        for end in 0..bytes.len() {
            let mut restored = value;
            assert!(restored.load(&mut Reader::new(&bytes[..end])).is_err());
        }
    }

    #[test]
    fn rtc_snapshot_round_trips_all_latches_and_calendar() {
        let rtc = Rtc {
            calendar: RtcDateTime::new([4, 3, 8, 2, 12, 34, 56]).unwrap(),
            selected: true,
            phase: Phase::Read {
                data: [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde],
                len: 7,
                bit: 19,
            },
            ..Rtc::default()
        };
        let bytes = encoded(&rtc);

        let mut restored = Rtc::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, rtc);
        assert_prefixes(rtc);

        let mut invalid = bytes;
        invalid[4] = 0x41;
        assert_eq!(
            Rtc::default().load(&mut Reader::new(&invalid)),
            Err(SnapshotError::Corrupt("RTC control"))
        );
    }

    #[test]
    fn rtc_register_and_phase_encodings_are_exhaustive_and_checked() {
        for register in [Register::Control, Register::DateTime, Register::Time] {
            let bytes = encoded(&register);
            let mut restored = Register::Control;
            restored.load(&mut Reader::new(&bytes)).unwrap();
            assert_eq!(restored, register);
            assert_prefixes(register);
        }

        let phases = [
            Phase::Command {
                byte: 0x06,
                bits: 4,
            },
            Phase::Write {
                register: Register::Time,
                data: [1, 2, 3, 4, 5, 6, 7],
                offset: 2,
                byte: 0x55,
                bits: 3,
            },
            Phase::Read {
                data: [7, 6, 5, 4, 3, 2, 1],
                len: 3,
                bit: 17,
            },
            Phase::Done,
        ];
        for phase in phases {
            let bytes = encoded(&phase);
            let mut restored = Phase::Done;
            restored.load(&mut Reader::new(&bytes)).unwrap();
            assert_eq!(restored, phase);
            assert_prefixes(phase);
        }

        let mut invalid_register = encoded(&Register::Control);
        invalid_register[4] = 0xff;
        assert_eq!(
            Register::Control.load(&mut Reader::new(&invalid_register)),
            Err(SnapshotError::Corrupt("RTC register"))
        );

        let mut invalid_phase = encoded(&Phase::Done);
        invalid_phase[4] = 0xff;
        assert_eq!(
            Phase::Done.load(&mut Reader::new(&invalid_phase)),
            Err(SnapshotError::Corrupt("RTC phase"))
        );
    }
}
