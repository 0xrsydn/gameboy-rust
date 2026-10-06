//! Edge-driven RTC command framing with latched reads and complete-payload writes.
use super::RtcDateTime;

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
