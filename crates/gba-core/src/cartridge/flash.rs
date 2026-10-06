//! Bounded Macronix Flash identification, array reads, and 128 KiB banking.
//! Program/erase commands remain diagnostic: this module cannot claim a completed save.
use crate::memory::MemoryError;
use std::{error::Error, fmt};

pub const SAVE_START: u32 = 0x0e00_0000;
pub const SAVE_END: u32 = 0x0e00_ffff;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SaveDevice {
    #[default]
    None,
    /// Macronix ID 1CC2h. Array reads and identification only.
    Flash64,
    /// Macronix ID 09C2h. Adds two 64 KiB banks; no programming or erase yet.
    Flash128,
}

impl SaveDevice {
    pub fn capacity(self) -> usize {
        match self {
            Self::None => 0,
            Self::Flash64 => 64 * 1024,
            Self::Flash128 => 128 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    NoDevice,
    InvalidSize { expected: usize, actual: usize },
}
impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no cartridge save device selected"),
            Self::InvalidSize { expected, actual } => write!(
                f,
                "save image size {actual}; expected exactly {expected} bytes"
            ),
        }
    }
}
impl Error for SaveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Flash {
    device: SaveDevice,
    phase: Phase,
    identify: bool,
    bank: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Unlock,
    Command,
    Bank,
}

impl Flash {
    pub(super) fn new(device: SaveDevice) -> Option<Self> {
        (device != SaveDevice::None).then_some(Self {
            device,
            phase: Phase::Ready,
            identify: false,
            bank: 0,
        })
    }

    pub(super) fn device(self) -> SaveDevice {
        self.device
    }

    pub(super) fn read(self, address: u32, image: &[u8]) -> Result<u8, MemoryError> {
        let offset = (address - SAVE_START) as usize;
        if self.identify {
            return match offset {
                0 => Ok(0xc2),
                1 => Ok(if self.device == SaveDevice::Flash64 {
                    0x1c
                } else {
                    9
                }),
                _ => Err(MemoryError::UnsupportedCartridgeAccess {
                    address,
                    operation: "Flash ID read outside manufacturer/device bytes",
                }),
            };
        }
        Ok(image[usize::from(self.bank) * 0x10000 + offset])
    }

    pub(super) fn write(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        let offset = address - SAVE_START;
        let error = |operation| MemoryError::UnsupportedIo {
            address,
            value,
            operation,
        };
        let mut next = *self;
        // GBATEK documents direct reset for the 64 KiB chip. For 128 KiB, accept
        // only a redundant reset while already idle in array mode. No busy/ID exit is inferred.
        if offset == 0x5555
            && value == 0xf0
            && (self.device == SaveDevice::Flash64
                || (self.phase == Phase::Ready && !self.identify))
        {
            next.identify = false;
            next.phase = Phase::Ready;
        } else {
            next.phase = match self.phase {
                Phase::Ready if offset == 0x5555 && value == 0xaa => Phase::Unlock,
                Phase::Unlock if offset == 0x2aaa && value == 0x55 => Phase::Command,
                Phase::Command if offset == 0x5555 => {
                    if self.identify && value != 0xf0 {
                        return Err(error("Flash command while in ID mode"));
                    }
                    match value {
                        0x90 => {
                            next.identify = true;
                            Phase::Ready
                        }
                        0xf0 => {
                            next.identify = false;
                            Phase::Ready
                        }
                        0xb0 if self.device == SaveDevice::Flash128 => Phase::Bank,
                        0xb0 => return Err(error("Flash bank selection on 64 KiB device")),
                        0xa0 => return Err(error("Flash byte programming")),
                        0x80 => return Err(error("Flash erase setup")),
                        _ => return Err(error("Flash command")),
                    }
                }
                Phase::Bank if offset == 0 && value < 2 => {
                    next.bank = value;
                    Phase::Ready
                }
                Phase::Bank => return Err(error("Flash bank address/value")),
                _ => return Err(error("Flash unlock/reset sequence")),
            };
        }
        *self = next;
        Ok(())
    }
}
