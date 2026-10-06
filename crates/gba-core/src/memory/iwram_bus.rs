//! Persistent local IWRAM lanes, independent of CPU state and executing region.
//! CPU instructions stage changes; successful DMA units commit accesses in order.
use std::cell::Cell;

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
