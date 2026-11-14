//! Persistent local IWRAM lanes, independent of CPU state and executing region.
//! CPU instructions stage changes; successful DMA units commit accesses in order.
use std::cell::Cell;

use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use crate::timing::AccessWidth;

#[derive(Clone, Copy, Default)]
struct Lanes {
    value: u32,
    known: u32,
}

impl Lanes {
    fn drive(&mut self, address: u32, width: AccessWidth, value: u32) {
        if address >> 24 != 3 {
            return;
        }
        debug_assert_eq!(address & (width.bytes() - 1), 0);
        let mask = match width {
            AccessWidth::Byte => 0xff,
            AccessWidth::Halfword => 0xffff,
            AccessWidth::Word => u32::MAX,
        };
        let shift = (address & 3) * 8;
        let mask = mask << shift;
        self.value = (self.value & !mask) | ((value << shift) & mask);
        self.known |= mask;
    }

    fn word(self) -> Option<u32> {
        (self.known == u32::MAX).then_some(self.value)
    }
}

#[derive(Default)]
pub(super) struct IwramBus {
    committed: Lanes,
    // Reads use &Memory. Stage their lane changes without changing committed history.
    pending: Cell<Option<Lanes>>,
}

impl IwramBus {
    #[cfg(test)]
    pub(super) fn committed_word(&self) -> Option<u32> {
        self.committed.word()
    }

    pub(super) fn begin(&mut self) {
        debug_assert!(self.pending.get().is_none());
        self.pending.set(Some(self.committed));
    }

    pub(super) fn snapshot(&self) -> Option<u32> {
        self.pending.get().and_then(Lanes::word)
    }

    pub(super) fn access(&self, address: u32, width: AccessWidth, value: u32) {
        if let Some(mut lanes) = self.pending.get() {
            lanes.drive(address, width, value);
            self.pending.set(Some(lanes));
        }
    }

    /// Committed lanes (value, known mask) between steps, for comparing bus history.
    pub(super) fn committed_lanes(&self) -> (u32, u32) {
        debug_assert!(self.pending.get().is_none());
        (self.committed.value, self.committed.known)
    }

    /// DMA runs between instructions. Call only after the entire unit succeeds.
    pub(super) fn dma_access(&mut self, address: u32, width: AccessWidth, value: u32) {
        debug_assert!(self.pending.get().is_none());
        self.committed.drive(address, width, value);
    }

    pub(super) fn finish(&mut self, succeeded: bool) {
        let pending = self.pending.take();
        if succeeded {
            self.committed = pending.expect("IWRAM transaction must have started");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_lanes_stay_unknown_until_all_bytes_have_been_driven() {
        let mut bus = IwramBus::default();
        bus.begin();
        bus.access(0x0300_0104, AccessWidth::Halfword, 0x1234);
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0300_2002, AccessWidth::Byte, 0x56);
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0200_2003, AccessWidth::Byte, 0xff);
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0300_2003, AccessWidth::Byte, 0x78);
        assert_eq!(bus.snapshot(), Some(0x7856_1234));
        bus.finish(true);
        bus.begin();
        bus.access(0x0300_0106, AccessWidth::Halfword, 0xabcd);
        assert_eq!(bus.snapshot(), Some(0xabcd_1234));
    }

    #[test]
    fn failed_transaction_discards_all_staged_accesses_not_only_prefetch() {
        let mut bus = IwramBus::default();
        bus.begin();
        bus.access(0x0300_0104, AccessWidth::Halfword, 0x1234);
        bus.access(0x0300_2000, AccessWidth::Word, 0x1122_3344);
        bus.finish(true);
        bus.begin();
        bus.access(0x0300_0106, AccessWidth::Halfword, 0xabcd);
        bus.access(0x0300_2000, AccessWidth::Word, 0xdead_beef);
        bus.access(0x0300_2001, AccessWidth::Byte, 0x55);
        assert_eq!(bus.snapshot(), Some(0xdead_55ef));
        bus.finish(false);
        bus.begin();
        bus.access(0x0300_0106, AccessWidth::Halfword, 0x5678);
        assert_eq!(bus.snapshot(), Some(0x5678_3344));
    }
}

impl Snapshot for IwramBus {
    fn save(&self, writer: &mut Writer) {
        let Self { committed, pending } = self;
        if pending.get().is_some() {
            writer.fail(SnapshotError::MidStep);
            return;
        }
        writer.tag(b"IWBS");
        let Lanes { value, known } = committed;
        writer.u32(*value);
        writer.u32(*known);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self { committed, pending } = self;
        reader.tag(b"IWBS")?;
        if pending.get().is_some() {
            return Err(SnapshotError::MidStep);
        }
        let value = reader.u32()?;
        let known = reader.u32()?;
        if value & !known != 0
            || [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000]
                .into_iter()
                .any(|lane| {
                    let bits = known & lane;
                    bits != 0 && bits != lane
                })
        {
            return Err(SnapshotError::Corrupt("IWRAM bus lanes"));
        }
        *committed = Lanes { value, known };
        pending.set(None);
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn populated_bus() -> IwramBus {
        let mut bus = IwramBus::default();
        bus.dma_access(0x0300_0000, AccessWidth::Byte, 0x12);
        bus.dma_access(0x0300_0002, AccessWidth::Halfword, 0x3456);
        bus
    }

    fn save(bus: &IwramBus) -> Vec<u8> {
        let mut writer = Writer::new();
        bus.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn iwram_bus_round_trips_committed_lane_history() {
        let source = populated_bus();
        let bytes = save(&source);
        let mut restored = IwramBus::default();
        restored.load(&mut Reader::new(&bytes)).unwrap();
        let mut writer = Writer::new();
        restored.save(&mut writer);
        assert_eq!(writer.finish().unwrap(), bytes);
    }

    #[test]
    fn every_strict_iwram_bus_prefix_is_rejected() {
        let bytes = save(&populated_bus());
        for end in 0..bytes.len() {
            let mut restored = IwramBus::default();
            assert!(
                restored.load(&mut Reader::new(&bytes[..end])).is_err(),
                "accepted prefix of length {end}"
            );
        }
    }

    #[test]
    fn iwram_bus_rejects_values_in_unknown_lanes() {
        let mut bytes = save(&populated_bus());
        bytes[5] |= 0x80;
        let mut restored = IwramBus::default();
        assert!(matches!(
            restored.load(&mut Reader::new(&bytes)),
            Err(SnapshotError::Corrupt(_))
        ));
    }

    #[test]
    fn iwram_bus_rejects_transaction_staging() {
        let mut bus = populated_bus();
        bus.begin();
        let mut writer = Writer::new();
        bus.save(&mut writer);
        assert_eq!(writer.finish(), Err(SnapshotError::MidStep));
    }
}
