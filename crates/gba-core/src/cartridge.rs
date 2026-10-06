//! Optional Game Pak hardware. No host clock, filesystem, or ROM-byte mutation.

use crate::memory::MemoryError;

pub const GPIO_DATA: u32 = 0x0800_00c4;
pub const GPIO_DIRECTION: u32 = 0x0800_00c6;
pub const GPIO_CONTROL: u32 = 0x0800_00c8;

/// Explicit peripheral selection; ROM contents do not select a device.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CartridgeHardware {
    #[default]
    None,
    /// GPIO and RTC command/control only. Calendar and interrupt commands remain diagnostic.
    Rtc,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cartridge {
    gpio: Option<Gpio>,
}

impl Cartridge {
    pub(crate) fn new(hardware: CartridgeHardware) -> Self {
        Self {
            gpio: (hardware == CartridgeHardware::Rtc).then(Gpio::default),
        }
    }

    pub(crate) fn mapped(&self, address: u32) -> bool {
        self.gpio.is_some() && (GPIO_DATA..GPIO_CONTROL + 2).contains(&address)
    }

    /// Disabled reads expose the supplied ROM bytes, not a zero-filled replacement.
    pub(crate) fn read8(&self, address: u32) -> Option<u8> {
        let gpio = self.gpio?;
        if !self.mapped(address) || !gpio.readable {
            return None;
        }
        let value = match address & !1 {
            GPIO_DATA => gpio.pins(),
            GPIO_DIRECTION => gpio.direction,
            GPIO_CONTROL => 1,
            _ => unreachable!(),
        };
        Some(if address & 1 == 0 { value } else { 0 })
    }

    /// Apply one access to a copy first. A rejected second halfword changes nothing.
    pub(crate) fn write(&mut self, address: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        if !self.mapped(address) {
            return Ok(()); // Non-cartridge accesses are validated by the bus.
        }
        let fail = |at, value, operation| MemoryError::UnsupportedIo {
            address: at,
            value,
            operation,
        };
        if bytes.len() == 1 {
            return Err(fail(address, bytes[0], "Game Pak GPIO byte write"));
        }
        let mut next = self.gpio.unwrap();
        for (i, pair) in bytes.chunks_exact(2).enumerate() {
            let at = address + i as u32 * 2;
            let value = pair[0]; // Registers use at most the low four bits.
            match at {
                GPIO_DATA | GPIO_DIRECTION => {
                    if at == GPIO_DATA {
                        next.latch = value & 15;
                    } else {
                        next.direction = value & 15;
                    }
                    next.rtc
                        .update(next.latch & next.direction, next.direction)
                        .map_err(|operation| fail(at, value, operation))?;
                }
                GPIO_CONTROL => next.readable = value & 1 != 0,
                _ => return Err(MemoryError::ReadOnly(at)),
            }
        }
        self.gpio = Some(next);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Gpio {
    latch: u8,
    direction: u8,
    readable: bool,
    rtc: Rtc,
}

impl Gpio {
    fn pins(self) -> u8 {
        // RTC input defaults: SIO high outside a read; SCK/CS/unused low.
        (self.latch & self.direction) | (u8::from(self.rtc.output) << 1 & !self.direction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rtc {
    control: u8,
    selected: bool,
    clock: bool,
    sample: bool,
    output: bool,
    phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Command { byte: u8, bits: u8 },
    WriteControl { byte: u8, bits: u8 },
    ReadControl { byte: u8, bits: u8 },
    Done,
}

impl Default for Rtc {
    fn default() -> Self {
        Self {
            control: 0x40, // Deterministic powered 24-hour mode, not a sampled host clock.
            selected: false,
            clock: true,
            sample: true,
            output: true,
            phase: Phase::Command { byte: 0, bits: 0 },
        }
    }
}

impl Rtc {
    fn update(&mut self, pins: u8, direction: u8) -> Result<(), &'static str> {
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
                Phase::Command { .. } | Phase::WriteControl { .. } => {
                    if direction & 2 == 0 {
                        return Err("RTC command/data write with SIO input");
                    }
                    self.sample = data;
                }
                Phase::ReadControl { byte, bits } if self.clock => {
                    if direction & 2 != 0 {
                        return Err("RTC read with SIO output");
                    }
                    self.output = byte & (1 << bits) != 0;
                    self.phase = if bits == 7 {
                        Phase::Done
                    } else {
                        Phase::ReadControl {
                            byte,
                            bits: bits + 1,
                        }
                    };
                }
                Phase::Done if self.clock => return Err("RTC clocks beyond command length"),
                _ => {}
            }
        } else if !self.clock {
            match self.phase {
                Phase::Command { byte, bits } | Phase::WriteControl { byte, bits } => {
                    if direction & 2 == 0 || data != self.sample {
                        return Err("RTC SIO change at sampling edge");
                    }
                    let byte = byte | (u8::from(self.sample) << bits);
                    let command = matches!(self.phase, Phase::Command { .. });
                    self.phase = if bits != 7 {
                        if command {
                            Phase::Command {
                                byte,
                                bits: bits + 1,
                            }
                        } else {
                            Phase::WriteControl {
                                byte,
                                bits: bits + 1,
                            }
                        }
                    } else if command {
                        self.command(byte)?
                    } else {
                        // Bits 1/3/5 lack an implemented IRQ/control model.
                        if byte & 0x2a != 0 {
                            return Err("RTC interrupt/unknown control bits");
                        }
                        self.control = byte & 0x40;
                        Phase::Done
                    };
                }
                _ => {}
            }
        }
        self.clock = clock;
        Ok(())
    }

    fn command(&mut self, byte: u8) -> Result<Phase, &'static str> {
        // The first four wire bits are 0,1,1,0. Translate GBATEK's LSB-first
        // representation to its MSB-first command numbering; parameters stay LSB first.
        if byte & 15 != 6 {
            return Err("RTC command encoding");
        }
        let command = byte.reverse_bits();
        match (command >> 1) & 7 {
            0 => {
                self.control = 0;
                Ok(Phase::Done)
            }
            1 if command & 1 != 0 => Ok(Phase::ReadControl {
                byte: self.control,
                bits: 0,
            }),
            1 => Ok(Phase::WriteControl { byte: 0, bits: 0 }),
            2 | 3 => Err("RTC calendar access"),
            6 => Err("RTC force interrupt"),
            _ => Err("RTC unused command"),
        }
    }
}
