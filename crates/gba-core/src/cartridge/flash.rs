//! Bounded Macronix Flash commands with nominal delayed program/erase completion.
use crate::memory::MemoryError;
use std::{error::Error, fmt};

pub const SAVE_START: u32 = 0x0e00_0000;
pub const SAVE_END: u32 = 0x0e00_ffff;
/// Nominal simulation delays, not measured Macronix operation times.
pub const FLASH_PROGRAM_CYCLES: u32 = 650;
pub const FLASH_ERASE_CYCLES: u32 = 30000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SaveDevice {
    #[default]
    None,
    /// Macronix ID 1CC2h, 64 KiB array.
    Flash64,
    /// Macronix ID 09C2h, two 64 KiB banks.
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
    pending: Option<Pending>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SaveMutation {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) value: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    mutation: SaveMutation,
    remaining: u32,
    poll: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Unlock,
    Command,
    Bank,
    Program,
    EraseUnlock,
    EraseCommand,
    EraseConfirm,
}

impl Flash {
    pub(super) fn new(device: SaveDevice) -> Option<Self> {
        (device != SaveDevice::None).then_some(Self {
            device,
            phase: Phase::Ready,
            identify: false,
            bank: 0,
            pending: None,
        })
    }

    pub(super) fn device(self) -> SaveDevice {
        self.device
    }

    pub(super) fn read(self, address: u32, image: &[u8]) -> Result<u8, MemoryError> {
        let offset = (address - SAVE_START) as usize;
        let index = usize::from(self.bank) * 0x10000 + offset;
        if let Some(pending) = self.pending {
            if pending.remaining != 0 {
                if address != pending.poll {
                    return Err(MemoryError::UnsupportedCartridgeAccess {
                        address,
                        operation: "Flash busy read outside polling address",
                    });
                }
                // Bounded DQ7-only data polling. Toggle/error bits are not modeled.
                return Ok((pending.mutation.value ^ 0x80) & 0x80);
            }
            if (pending.mutation.start..pending.mutation.end).contains(&index) {
                return Ok(pending.mutation.value);
            }
        }
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
        Ok(image[index])
    }

    pub(super) fn advance(&mut self, cycles: u32) {
        if let Some(pending) = self.pending.as_mut() {
            pending.remaining = pending.remaining.saturating_sub(cycles);
        }
    }

    pub(super) fn unfinished(self) -> bool {
        self.pending.is_some() || self.phase != Phase::Ready
    }

    pub(super) fn take_mutation(&mut self) -> Option<SaveMutation> {
        if self.pending.is_some_and(|p| p.remaining == 0) {
            self.pending.take().map(|p| p.mutation)
        } else {
            None
        }
    }

    pub(super) fn write(
        &mut self,
        address: u32,
        value: u8,
        image: &[u8],
    ) -> Result<(), MemoryError> {
        let offset = address - SAVE_START;
        let error = |operation| MemoryError::UnsupportedIo {
            address,
            value,
            operation,
        };
        let mut next = *self;
        if self.pending.is_some_and(|p| p.remaining != 0) {
            if self.device == SaveDevice::Flash64 && offset == 0x5555 && value == 0xf0 {
                next.pending = None;
                next.phase = Phase::Ready;
                *self = next;
                return Ok(());
            }
            return Err(error("Flash write during busy operation"));
        }
        // GBATEK documents direct reset for the 64 KiB chip. For 128 KiB, accept
        // only a redundant reset while already idle in array mode. No busy/ID exit is inferred.
        if self.phase != Phase::Program
            && offset == 0x5555
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
                        0xa0 => Phase::Program,
                        0x80 => Phase::EraseUnlock,
                        _ => return Err(error("Flash command")),
                    }
                }
                Phase::Bank if offset == 0 && value < 2 => {
                    next.bank = value;
                    Phase::Ready
                }
                Phase::Bank => return Err(error("Flash bank address/value")),
                Phase::Program => {
                    if self.pending.is_some() {
                        return Err(error("Flash completion awaiting commit"));
                    }
                    let index = usize::from(self.bank) * 0x10000 + offset as usize;
                    if value & !image[index] != 0 {
                        return Err(error(
                            "Flash programming requires erase for zero-to-one bits",
                        ));
                    }
                    next.pending = Some(Pending {
                        mutation: SaveMutation {
                            start: index,
                            end: index + 1,
                            value,
                        },
                        remaining: FLASH_PROGRAM_CYCLES,
                        poll: address,
                    });
                    Phase::Ready
                }
                Phase::EraseUnlock if offset == 0x5555 && value == 0xaa => Phase::EraseCommand,
                Phase::EraseCommand if offset == 0x2aaa && value == 0x55 => Phase::EraseConfirm,
                Phase::EraseConfirm => {
                    if self.pending.is_some() {
                        return Err(error("Flash completion awaiting commit"));
                    }
                    let (start, end, poll) = match value {
                        0x10 if offset == 0x5555 => (0, self.device.capacity(), SAVE_START),
                        0x30 if offset & 0xfff == 0 => {
                            let start = usize::from(self.bank) * 0x10000 + offset as usize;
                            (start, start + 0x1000, address)
                        }
                        _ => return Err(error("Flash erase confirmation/address")),
                    };
                    next.pending = Some(Pending {
                        mutation: SaveMutation {
                            start,
                            end,
                            value: 0xff,
                        },
                        remaining: FLASH_ERASE_CYCLES,
                        poll,
                    });
                    Phase::Ready
                }
                _ => return Err(error("Flash unlock/reset sequence")),
            };
        }
        *self = next;
        Ok(())
    }
}
