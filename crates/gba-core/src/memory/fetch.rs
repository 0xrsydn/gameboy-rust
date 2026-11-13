//! One mapped sample supplies instruction bits and the supported bus observation.
//! Sampling is side-effect-free; execution commits history under the diagnostic policy.
use super::{InstructionSet, Memory, MemoryError};
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstructionFetch {
    pub(super) address: u32,
    pub(super) state: InstructionSet,
    pub(crate) instruction: Result<u32, MemoryError>,
    // None for unsupported regions, failed reads, or lane-based Thumb IWRAM history.
    pub(super) bus_word: Option<u32>,
}

impl InstructionFetch {
    pub(crate) fn address(&self) -> u32 {
        self.address
    }
}

impl Memory {
    pub(crate) fn fetch_instruction(
        &self,
        address: u32,
        state: InstructionSet,
    ) -> InstructionFetch {
        let instruction = self.read_instruction(address, state);
        let bus_word = instruction.as_ref().ok().copied().and_then(|value| {
            if state == InstructionSet::Arm {
                return Some(value);
            }
            match address >> 24 {
                // BIOS/OAM drive the aligned word. Read only the other halfword;
                // do not sample the instruction bytes a second time.
                0x00 | 0x07 => {
                    let other = self
                        .read_instruction(address ^ 2, InstructionSet::Thumb)
                        .ok()?;
                    Some(if address & 2 == 0 {
                        value | (other << 16)
                    } else {
                        other | (value << 16)
                    })
                }
                // IWRAM updates its existing addressed lanes at execution entry.
                0x03 => None,
                0x02 | 0x05 | 0x06 | 0x08..=0x0d => Some(value * 0x0001_0001),
                _ => None,
            }
        });
        InstructionFetch {
            address,
            state,
            instruction,
            bus_word,
        }
    }

    /// Strict mapped bytes, without data context, timing, or bus-history changes.
    /// An unavailable wider bus observation must not invalidate a valid instruction.
    fn read_instruction(&self, address: u32, state: InstructionSet) -> Result<u32, MemoryError> {
        let width = state.width() as usize;
        if address & (width as u32 - 1) != 0 {
            return Err(MemoryError::Unaligned(address));
        }
        if address >> 24 >= 0x08 && self.with_cartridge(|c| c.save_mapped(address)) {
            return Err(MemoryError::UnsupportedCartridgeAccess {
                address,
                operation: "Flash instruction fetch",
            });
        }
        match state {
            InstructionSet::Arm => {
                if let Some(bytes) = self.plain_bytes::<4>(address) {
                    return Ok(u32::from_le_bytes(bytes));
                }
            }
            InstructionSet::Thumb => {
                if let Some(bytes) = self.plain_bytes::<2>(address) {
                    return Ok(u32::from(u16::from_le_bytes(bytes)));
                }
            }
        }
        let mut bytes = [0; 4];
        for (offset, byte) in bytes[..width].iter_mut().enumerate() {
            *byte = self.read_mapped_byte(address.wrapping_add(offset as u32))?;
        }
        Ok(u32::from_le_bytes(bytes))
    }
}

impl Snapshot for InstructionFetch {
    fn save(&self, writer: &mut Writer) {
        let Self {
            address,
            state,
            instruction,
            bus_word,
        } = self;
        writer.tag(b"FETC");
        writer.u32(*address);
        writer.u8(match state {
            InstructionSet::Arm => 0,
            InstructionSet::Thumb => 1,
        });
        save_fetch_result(instruction, writer);
        writer.option(bus_word.as_ref(), |word, writer| writer.u32(*word));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            address,
            state,
            instruction,
            bus_word,
        } = self;
        reader.tag(b"FETC")?;
        *address = reader.u32()?;
        *state = match reader.u8()? {
            0 => InstructionSet::Arm,
            1 => InstructionSet::Thumb,
            _ => return Err(SnapshotError::Corrupt("instruction set")),
        };
        *instruction = load_fetch_result(reader)?;
        *bus_word = reader.option(|reader| reader.u32())?;
        Ok(())
    }
}

/// A deferred fetch diagnostic is stored beside the instruction it would have
/// produced. Address-only errors are encoded; errors that carry a static
/// description string are not, and fail the save.
pub(crate) fn save_fetch_result(instruction: &Result<u32, MemoryError>, writer: &mut Writer) {
    match instruction {
        Ok(value) => {
            writer.u8(0);
            writer.u32(*value);
        }
        Err(MemoryError::Unmapped(address)) => {
            writer.u8(1);
            writer.u32(*address);
        }
        Err(MemoryError::ReadOnly(address)) => {
            writer.u8(2);
            writer.u32(*address);
        }
        Err(MemoryError::Unaligned(address)) => {
            writer.u8(3);
            writer.u32(*address);
        }
        Err(_) => {
            writer.u8(0xff);
            writer.fail(SnapshotError::Unsupported("deferred fetch diagnostic"));
        }
    }
}

pub(crate) fn load_fetch_result(
    reader: &mut Reader<'_>,
) -> Result<Result<u32, MemoryError>, SnapshotError> {
    Ok(match reader.u8()? {
        0 => Ok(reader.u32()?),
        1 => Err(MemoryError::Unmapped(reader.u32()?)),
        2 => Err(MemoryError::ReadOnly(reader.u32()?)),
        3 => Err(MemoryError::Unaligned(reader.u32()?)),
        _ => return Err(SnapshotError::Corrupt("fetch result")),
    })
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn populated_fetch() -> InstructionFetch {
        InstructionFetch {
            address: 0x0800_0022,
            state: InstructionSet::Thumb,
            instruction: Ok(0x46c0),
            bus_word: Some(0x1234_46c0),
        }
    }

    fn save(fetch: &InstructionFetch) -> Vec<u8> {
        let mut writer = Writer::new();
        fetch.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn instruction_fetch_round_trips_all_fields() {
        let source = populated_fetch();
        let bytes = save(&source);
        let mut restored = InstructionFetch {
            address: 0,
            state: InstructionSet::Arm,
            instruction: Ok(0),
            bus_word: None,
        };
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, source);
    }

    #[test]
    fn every_strict_fetch_prefix_is_rejected() {
        let bytes = save(&populated_fetch());
        for end in 0..bytes.len() {
            let mut restored = populated_fetch();
            assert!(
                restored.load(&mut Reader::new(&bytes[..end])).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn instruction_fetch_rejects_unknown_instruction_set() {
        let mut bytes = save(&populated_fetch());
        bytes[8] = 0xff;
        let mut restored = populated_fetch();
        assert!(matches!(
            restored.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt(_))
        ));
    }

    #[test]
    fn address_diagnostics_round_trip_and_described_ones_cannot_be_saved() {
        for error in [
            MemoryError::Unmapped(0x0200_0000),
            MemoryError::ReadOnly(0x0800_0000),
            MemoryError::Unaligned(0x0300_0001),
        ] {
            let source = InstructionFetch {
                instruction: Err(error),
                ..populated_fetch()
            };
            let bytes = save(&source);
            let mut restored = populated_fetch();
            restored.load(&mut Reader::new(&bytes)).unwrap();
            assert_eq!(restored, source);
        }
        let fetch = InstructionFetch {
            instruction: Err(MemoryError::UnsupportedIo {
                address: 0x0400_0000,
                value: 0,
                operation: "write",
            }),
            ..populated_fetch()
        };
        let mut writer = Writer::new();
        fetch.save(&mut writer);
        assert_eq!(
            writer.finish(),
            Err(SnapshotError::Unsupported("deferred fetch diagnostic"))
        );
    }
}
