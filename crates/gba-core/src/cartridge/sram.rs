//! Battery-backed SRAM: a 32 KiB byte-wide array with no command protocol.
//! CPU and DMA stores are staged as byte mutations and committed in order when their
//! step completes. Wider accesses use the 8-bit bus byte lanes (see `Memory::store32`).

use super::{SaveDevice, SaveMutation, SAVE_START};
use crate::memory::MemoryError;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

/// The 32 KiB array repeats through the save regions.
const MIRROR_MASK: u32 = 0x7fff;
/// One step stores at most sixteen words (`STM` with every register), one byte each.
const MAX_PENDING: usize = 16;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sram {
    /// Staged (index, value) byte stores in program order.
    pending: [(u16, u8); MAX_PENDING],
    count: u8,
}

impl Sram {
    fn index(address: u32) -> usize {
        ((address - SAVE_START) & MIRROR_MASK) as usize
    }

    pub(super) fn read(self, address: u32, image: &[u8]) -> u8 {
        let index = Self::index(address) as u16;
        self.pending[..usize::from(self.count)]
            .iter()
            .rev()
            .find(|(pending, _)| *pending == index)
            .map_or(image[usize::from(index)], |&(_, value)| value)
    }

    pub(super) fn write(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        if usize::from(self.count) == MAX_PENDING {
            return Err(MemoryError::UnsupportedCartridgeAccess {
                address,
                operation: "more than sixteen SRAM writes before commit",
            });
        }
        self.pending[usize::from(self.count)] = (Self::index(address) as u16, value);
        self.count += 1;
        Ok(())
    }

    pub(super) fn unfinished(self) -> bool {
        self.count != 0
    }

    /// The oldest staged store; call until `None` to commit every store in order.
    pub(super) fn take_mutation(&mut self) -> Option<SaveMutation> {
        if self.count == 0 {
            return None;
        }
        let (index, value) = self.pending[0];
        self.pending.copy_within(1..usize::from(self.count), 0);
        self.count -= 1;
        self.pending[usize::from(self.count)] = (0, 0);
        let start = usize::from(index);
        Some(SaveMutation {
            start,
            end: start + 1,
            value,
        })
    }
}

impl Snapshot for Sram {
    fn save(&self, writer: &mut Writer) {
        let Self { pending, count } = self;
        writer.tag(b"SRAM");
        writer.u8(*count);
        for &(index, value) in &pending[..usize::from(*count)] {
            writer.u16(index);
            writer.u8(value);
        }
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        reader.tag(b"SRAM")?;
        let count = reader.u8()?;
        if usize::from(count) > MAX_PENDING {
            return Err(SnapshotError::Corrupt("SRAM mutation count"));
        }
        let mut pending = [(0, 0); MAX_PENDING];
        for slot in &mut pending[..usize::from(count)] {
            let index = reader.u16()?;
            if usize::from(index) >= SaveDevice::Sram.capacity() {
                return Err(SnapshotError::Corrupt("SRAM mutation range"));
            }
            *slot = (index, reader.u8()?);
        }
        *self = Self { pending, count };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_stage_mirrored_bytes_in_order_and_reads_see_the_latest() {
        let image = vec![0xff; SaveDevice::Sram.capacity()];
        let mut sram = Sram::default();
        sram.write(SAVE_START + 0x8001, 0x42).unwrap(); // Mirror of offset 1.
        assert_eq!(sram.read(SAVE_START + 1, &image), 0x42);
        assert_eq!(sram.read(SAVE_START + 0x8001, &image), 0x42);
        assert_eq!(sram.read(SAVE_START, &image), 0xff);
        assert!(sram.unfinished());
        sram.write(SAVE_START + 1, 0x43).unwrap(); // A later store to the same byte wins.
        assert_eq!(sram.read(SAVE_START + 1, &image), 0x43);
        let first = sram.take_mutation().unwrap();
        assert_eq!((first.start, first.end, first.value), (1, 2, 0x42));
        let second = sram.take_mutation().unwrap();
        assert_eq!((second.start, second.end, second.value), (1, 2, 0x43));
        assert_eq!(sram.take_mutation(), None);
        assert!(!sram.unfinished());
    }

    #[test]
    fn sixteen_stores_fit_one_step_and_a_seventeenth_is_diagnostic() {
        let mut sram = Sram::default();
        for offset in 0..16 {
            sram.write(SAVE_START + offset * 4, offset as u8).unwrap();
        }
        assert!(sram.write(SAVE_START, 0).is_err());
    }

    #[test]
    fn snapshot_round_trips_and_rejects_out_of_range_mutations() {
        let mut sram = Sram::default();
        sram.write(SAVE_START + 0x7fff, 7).unwrap();
        sram.write(SAVE_START + 3, 9).unwrap();
        let mut writer = Writer::new();
        sram.save(&mut writer);
        let bytes = writer.finish().unwrap();
        let mut restored = Sram::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(restored, sram);

        let mut corrupt = Sram::default();
        corrupt.pending[0] = (0x8000, 0);
        corrupt.count = 1;
        let mut writer = Writer::new();
        corrupt.save(&mut writer);
        let bytes = writer.finish().unwrap();
        assert_eq!(
            Sram::default().load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt("SRAM mutation range"))
        );
    }
}
