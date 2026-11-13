//! Bounded Macronix Flash commands with nominal delayed program/erase completion.
use crate::memory::MemoryError;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use std::{error::Error, fmt};

pub const SAVE_START: u32 = 0x0e00_0000;
pub const SAVE_END: u32 = 0x0e00_ffff;
/// SRAM answers throughout 0x0e000000-0x0fffffff, repeating every 32 KiB.
pub const SRAM_MIRROR_END: u32 = 0x0fff_ffff;
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
    /// 32 KiB battery-backed SRAM, byte addressed and mirrored through the save window.
    Sram,
    /// 512-byte serial EEPROM with 6-bit block addresses, in the 0x0d window.
    Eeprom512,
    /// 8 KiB serial EEPROM with 14-bit block addresses, in the 0x0d window.
    Eeprom8k,
}

impl SaveDevice {
    pub fn capacity(self) -> usize {
        match self {
            Self::None => 0,
            Self::Flash64 => 64 * 1024,
            Self::Flash128 => 128 * 1024,
            Self::Sram => 32 * 1024,
            Self::Eeprom512 => 512,
            Self::Eeprom8k => 8 * 1024,
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
        matches!(device, SaveDevice::Flash64 | SaveDevice::Flash128).then_some(Self {
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

impl SaveMutation {
    fn read(reader: &mut Reader<'_>) -> Result<Self, SnapshotError> {
        reader.tag(b"SMUT")?;
        let start = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("Flash mutation range"))?;
        let end = usize::try_from(reader.u64()?)
            .map_err(|_| SnapshotError::Corrupt("Flash mutation range"))?;
        let value = reader.u8()?;
        let mutation = Self { start, end, value };
        if !mutation.valid_for(SaveDevice::Flash128.capacity()) {
            return Err(SnapshotError::Corrupt("Flash mutation range"));
        }
        Ok(mutation)
    }

    fn valid_for(self, capacity: usize) -> bool {
        let Self { start, end, value } = self;
        let Some(length) = end.checked_sub(start) else {
            return false;
        };
        if start >= end || end > capacity {
            return false;
        }
        match length {
            1 => true,
            0x1000 => value == 0xff && start & 0xfff == 0,
            0x1_0000 | 0x2_0000 => value == 0xff && start == 0,
            _ => false,
        }
    }
}

impl Snapshot for SaveMutation {
    fn save(&self, writer: &mut Writer) {
        let Self { start, end, value } = self;
        writer.tag(b"SMUT");
        writer.u64(*start as u64);
        writer.u64(*end as u64);
        writer.u8(*value);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { start, end, value } = self;
        let decoded = Self::read(reader)?;
        *start = decoded.start;
        *end = decoded.end;
        *value = decoded.value;
        Ok(())
    }
}

#[cfg(test)]
mod mutation_snapshot_tests {
    use super::*;

    #[test]
    fn save_mutation_snapshot_round_trips_and_rejects_prefixes() {
        let mutation = SaveMutation {
            start: 0x1000,
            end: 0x2000,
            value: 0xff,
        };
        let mut writer = Writer::new();
        mutation.save(&mut writer);
        let bytes = writer.finish().unwrap();

        let mut restored = SaveMutation {
            start: 0,
            end: 1,
            value: 0,
        };
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, mutation);
        for end in 0..bytes.len() {
            let mut target = SaveMutation {
                start: 0,
                end: 1,
                value: 0,
            };
            assert!(target.load(&mut Reader::new(&bytes[..end])).is_err());
        }
    }
}

impl Snapshot for Phase {
    fn save(&self, writer: &mut Writer) {
        writer.tag(b"FLPH");
        writer.u8(match self {
            Self::Ready => 0,
            Self::Unlock => 1,
            Self::Command => 2,
            Self::Bank => 3,
            Self::Program => 4,
            Self::EraseUnlock => 5,
            Self::EraseCommand => 6,
            Self::EraseConfirm => 7,
        });
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"FLPH")?;
        let decoded = match reader.u8()? {
            0 => Self::Ready,
            1 => Self::Unlock,
            2 => Self::Command,
            3 => Self::Bank,
            4 => Self::Program,
            5 => Self::EraseUnlock,
            6 => Self::EraseCommand,
            7 => Self::EraseConfirm,
            _ => return Err(SnapshotError::Corrupt("Flash phase")),
        };
        let _current = match self {
            Self::Ready => 0,
            Self::Unlock => 1,
            Self::Command => 2,
            Self::Bank => 3,
            Self::Program => 4,
            Self::EraseUnlock => 5,
            Self::EraseCommand => 6,
            Self::EraseConfirm => 7,
        };
        *self = decoded;
        Ok(())
    }
}

impl Pending {
    fn read(reader: &mut Reader<'_>) -> Result<Self, SnapshotError> {
        reader.tag(b"PEND")?;
        let mutation = SaveMutation::read(reader)?;
        let remaining = reader.u32()?;
        let poll = reader.u32()?;
        let pending = Self {
            mutation,
            remaining,
            poll,
        };
        if !pending.valid_for(SaveDevice::Flash128.capacity()) {
            return Err(SnapshotError::Corrupt("Flash pending operation"));
        }
        Ok(pending)
    }

    fn valid_for(self, capacity: usize) -> bool {
        let Self {
            mutation,
            remaining,
            poll,
        } = self;
        let SaveMutation { start, end, value } = mutation;
        let Some(length) = end.checked_sub(start) else {
            return false;
        };
        let Some(poll_offset) = poll
            .checked_sub(SAVE_START)
            .and_then(|offset| usize::try_from(offset).ok())
        else {
            return false;
        };
        if !mutation.valid_for(capacity) || poll_offset > 0xffff || remaining == 0 {
            return false;
        }
        let bank_offset = start % 0x1_0000;
        match length {
            1 => remaining <= FLASH_PROGRAM_CYCLES && poll_offset == bank_offset,
            0x1000 => {
                remaining <= FLASH_ERASE_CYCLES
                    && value == 0xff
                    && bank_offset & 0xfff == 0
                    && poll_offset == bank_offset
            }
            0x1_0000 | 0x2_0000 => {
                remaining <= FLASH_ERASE_CYCLES && value == 0xff && start == 0 && poll == SAVE_START
            }
            _ => false,
        }
    }
}

impl Snapshot for Pending {
    fn save(&self, writer: &mut Writer) {
        let Self {
            mutation,
            remaining,
            poll,
        } = self;
        writer.tag(b"PEND");
        mutation.save(writer);
        writer.u32(*remaining);
        writer.u32(*poll);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            mutation,
            remaining,
            poll,
        } = self;
        let decoded = Self::read(reader)?;
        *mutation = decoded.mutation;
        *remaining = decoded.remaining;
        *poll = decoded.poll;
        Ok(())
    }
}

impl Snapshot for Flash {
    fn save(&self, writer: &mut Writer) {
        let Self {
            device,
            phase,
            identify,
            bank,
            pending,
        } = self;
        writer.tag(b"FLAS");
        writer.u8(match device {
            SaveDevice::None => 0,
            SaveDevice::Flash64 => 1,
            SaveDevice::Flash128 => 2,
            SaveDevice::Sram | SaveDevice::Eeprom512 | SaveDevice::Eeprom8k => {
                unreachable!("only Flash devices have Flash state")
            }
        });
        phase.save(writer);
        writer.bool(*identify);
        writer.u8(*bank);
        writer.option(pending.as_ref(), |pending, writer| pending.save(writer));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            device,
            phase,
            identify,
            bank,
            pending,
        } = self;
        reader.tag(b"FLAS")?;
        let saved_device = match reader.u8()? {
            0 => SaveDevice::None,
            1 => SaveDevice::Flash64,
            2 => SaveDevice::Flash128,
            _ => return Err(SnapshotError::Corrupt("save device")),
        };
        if saved_device == SaveDevice::None {
            return Err(SnapshotError::Corrupt("save device"));
        }
        if saved_device != *device {
            return Err(SnapshotError::ConfigMismatch("save device"));
        }
        let mut saved_phase = Phase::Ready;
        saved_phase.load(reader)?;
        let saved_identify = reader.bool()?;
        let saved_bank = reader.u8()?;
        let saved_pending = reader.option(Pending::read)?;

        if saved_bank > 1 || (saved_device == SaveDevice::Flash64 && saved_bank != 0) {
            return Err(SnapshotError::Corrupt("Flash bank"));
        }
        if saved_device == SaveDevice::Flash64 && saved_phase == Phase::Bank {
            return Err(SnapshotError::Corrupt("Flash bank phase"));
        }
        if let Some(saved_pending) = saved_pending {
            let pending_length = saved_pending.mutation.end - saved_pending.mutation.start;
            let mutation_start = saved_pending.mutation.start;
            if !saved_pending.valid_for(saved_device.capacity())
                || saved_phase != Phase::Ready
                || saved_identify
                || ((pending_length == 1 || pending_length == 0x1000)
                    && mutation_start / 0x1_0000 != usize::from(saved_bank))
                || (matches!(pending_length, 0x1_0000 | 0x2_0000)
                    && pending_length != saved_device.capacity())
            {
                return Err(SnapshotError::Corrupt("Flash pending state"));
            }
        }

        *phase = saved_phase;
        *identify = saved_identify;
        *bank = saved_bank;
        *pending = saved_pending;
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

    fn erase_pending(start: usize, end: usize, remaining: u32, poll: u32) -> Pending {
        Pending {
            mutation: SaveMutation {
                start,
                end,
                value: 0xff,
            },
            remaining,
            poll,
        }
    }

    #[test]
    fn flash_snapshot_round_trips_controller_and_pending_operation() {
        let flash = Flash {
            device: SaveDevice::Flash128,
            phase: Phase::Ready,
            identify: false,
            bank: 1,
            pending: Some(erase_pending(
                0x1_1000,
                0x1_2000,
                12_345,
                SAVE_START + 0x1000,
            )),
        };
        let bytes = encoded(&flash);
        let mut restored = Flash::new(SaveDevice::Flash128).unwrap();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, flash);
        assert_prefixes(flash);

        let command_state = Flash {
            device: SaveDevice::Flash128,
            phase: Phase::Command,
            identify: true,
            bank: 1,
            pending: None,
        };
        let command_bytes = encoded(&command_state);
        let mut restored = Flash::new(SaveDevice::Flash128).unwrap();
        restored.load(&mut Reader::new(&command_bytes)).unwrap();
        assert_eq!(restored, command_state);

        let mut other_device = Flash::new(SaveDevice::Flash64).unwrap();
        assert_eq!(
            other_device.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::ConfigMismatch("save device"))
        );
    }

    #[test]
    fn flash_pending_phase_and_bank_encodings_are_checked() {
        let phases = [
            Phase::Ready,
            Phase::Unlock,
            Phase::Command,
            Phase::Bank,
            Phase::Program,
            Phase::EraseUnlock,
            Phase::EraseCommand,
            Phase::EraseConfirm,
        ];
        for phase in phases {
            let bytes = encoded(&phase);
            let mut restored = Phase::Ready;
            restored.load(&mut Reader::new(&bytes)).unwrap();
            assert_eq!(restored, phase);
            assert_prefixes(phase);
        }

        let pending = erase_pending(0x1000, 0x2000, 10_000, SAVE_START + 0x1000);
        let bytes = encoded(&pending);
        let mut restored = erase_pending(0, 1, 1, SAVE_START);
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, pending);
        assert_prefixes(pending);

        let mut invalid_phase = encoded(&Phase::Ready);
        invalid_phase[4] = 0xff;
        assert_eq!(
            Phase::Ready.load(&mut Reader::new(&invalid_phase)),
            Err(SnapshotError::Corrupt("Flash phase"))
        );

        let mut invalid_pending = encoded(&pending);
        invalid_pending[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            erase_pending(0, 1, 1, SAVE_START).load(&mut Reader::new(&invalid_pending)),
            Err(SnapshotError::Corrupt(_))
        ));

        let mut invalid_bank = encoded(&Flash::new(SaveDevice::Flash128).unwrap());
        invalid_bank[11] = 2;
        assert_eq!(
            Flash::new(SaveDevice::Flash128)
                .unwrap()
                .load(&mut Reader::new(&invalid_bank)),
            Err(SnapshotError::Corrupt("Flash bank"))
        );
    }
}
