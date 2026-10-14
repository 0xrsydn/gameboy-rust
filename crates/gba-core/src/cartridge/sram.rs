//! Battery-backed SRAM: a 32 KiB byte-wide array with no command protocol.
//! A CPU byte store is staged as a one-byte mutation and committed when its step completes.
//! Halfword and word accesses remain diagnostic until their 8-bit bus behavior is verified.

use super::{SaveDevice, SaveMutation, SAVE_START};
use crate::memory::MemoryError;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

/// The 32 KiB array repeats through the 64 KiB save window.
const MIRROR_MASK: u32 = 0x7fff;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sram {
    pending: Option<SaveMutation>,
}

impl Sram {
    fn index(address: u32) -> usize {
        ((address - SAVE_START) & MIRROR_MASK) as usize
    }

    pub(super) fn read(self, address: u32, image: &[u8]) -> u8 {
        let index = Self::index(address);
        match self.pending {
            Some(mutation) if (mutation.start..mutation.end).contains(&index) => mutation.value,
            _ => image[index],
        }
    }

    pub(super) fn write(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        if self.pending.is_some() {
            // One CPU step performs at most one byte store; DMA cannot write bytes.
            return Err(MemoryError::UnsupportedCartridgeAccess {
                address,
                operation: "second SRAM write before commit",
            });
        }
        let start = Self::index(address);
        self.pending = Some(SaveMutation {
            start,
            end: start + 1,
            value,
        });
        Ok(())
    }

    pub(super) fn unfinished(self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn take_mutation(&mut self) -> Option<SaveMutation> {
        self.pending.take()
    }
}

impl Snapshot for Sram {
    fn save(&self, writer: &mut Writer) {
        let Self { pending } = self;
        writer.tag(b"SRAM");
        writer.option(pending.as_ref(), |pending, writer| pending.save(writer));
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { pending } = self;
        reader.tag(b"SRAM")?;
        let decoded = reader.option(|reader| {
            let mut mutation = SaveMutation {
                start: 0,
                end: 0,
                value: 0,
            };
            mutation.load(reader)?;
            Ok(mutation)
        })?;
        if decoded.is_some_and(|m| m.end != m.start + 1 || m.end > SaveDevice::Sram.capacity()) {
            return Err(SnapshotError::Corrupt("SRAM mutation range"));
        }
        *pending = decoded;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_stage_one_mirrored_byte_and_reads_see_it_before_commit() {
        let image = vec![0xff; SaveDevice::Sram.capacity()];
        let mut sram = Sram::default();
        sram.write(SAVE_START + 0x8001, 0x42).unwrap(); // Mirror of offset 1.
        assert_eq!(sram.read(SAVE_START + 1, &image), 0x42);
        assert_eq!(sram.read(SAVE_START + 0x8001, &image), 0x42);
        assert_eq!(sram.read(SAVE_START, &image), 0xff);
        assert!(sram.unfinished());
        assert!(sram.write(SAVE_START, 0).is_err());
        let mutation = sram.take_mutation().unwrap();
        assert_eq!((mutation.start, mutation.end, mutation.value), (1, 2, 0x42));
        assert!(!sram.unfinished());
    }

    #[test]
    fn snapshot_round_trips_and_rejects_out_of_range_mutations() {
        let mut sram = Sram::default();
        sram.write(SAVE_START + 0x7fff, 7).unwrap();
        let mut writer = Writer::new();
        sram.save(&mut writer);
        let bytes = writer.finish().unwrap();
        let mut restored = Sram::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, sram);

        let mut writer = Writer::new();
        Sram {
            pending: Some(SaveMutation {
                start: 0x8000,
                end: 0x8001,
                value: 0,
            }),
        }
        .save(&mut writer);
        let bytes = writer.finish().unwrap();
        assert_eq!(
            Sram::default().load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt("SRAM mutation range"))
        );
    }
}
