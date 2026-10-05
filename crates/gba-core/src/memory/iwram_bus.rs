//! Transactional lane history for Thumb IWRAM execution and target-pair refills.
//! Completed DMA accesses update existing continuation lanes before the next fetch.
//! Unknown startup history is not replaced with guessed memory bytes.
use std::cell::Cell;

use crate::timing::AccessWidth;

#[derive(Clone, Copy, Default)]
struct Lanes {
    value: u32,
    known: u32,
}

impl Lanes {
    fn drive(&mut self, address: u32, width: AccessWidth, value: u32) {
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

#[derive(Clone, Copy)]
struct History {
    next_pc: u32,
    lanes: Lanes,
}

#[derive(Default)]
pub(super) struct IwramBus {
    committed: Option<History>,
    // Reads use &Memory. Stage their lane changes without changing committed history.
    pending: Cell<Option<History>>,
}

impl IwramBus {
    pub(super) fn begin(&mut self, pc: Option<u32>, fetched: Option<u16>) {
        debug_assert!(self.pending.get().is_none());
        self.pending.set(pc.map(|pc| {
            let mut lanes = self
                .committed
                .filter(|history| history.next_pc == pc)
                .map_or_else(Lanes::default, |history| history.lanes);
            if let Some(half) = fetched {
                lanes.drive(pc + 4, AccessWidth::Halfword, u32::from(half));
            } else {
                lanes = Lanes::default(); // Unavailable or region-crossing prefetch.
            }
            History {
                next_pc: pc.wrapping_add(2),
                lanes,
            }
        }));
    }

    pub(super) fn snapshot(&self) -> Option<u32> {
        self.pending.get().and_then(|history| history.lanes.word())
    }

    pub(super) fn access(&self, address: u32, width: AccessWidth, value: u32) {
        if address >> 24 != 3 {
            return;
        }
        if let Some(mut history) = self.pending.get() {
            history.lanes.drive(address, width, value);
            self.pending.set(Some(history));
        }
    }

    /// DMA runs between instructions. Commit only after the entire unit succeeds.
    /// Keep the expected CPU continuation PC; do not invent one at cold startup.
    pub(super) fn dma_access(&mut self, address: u32, width: AccessWidth, value: u32) {
        debug_assert!(self.pending.get().is_none());
        if address >> 24 == 3 {
            if let Some(history) = &mut self.committed {
                history.lanes.drive(address, width, value);
            }
        }
    }

    pub(super) fn finish(&mut self, succeeded: bool, sequential: bool) {
        let pending = self.pending.take();
        if succeeded {
            self.committed = if sequential {
                pending.filter(|history| history.next_pc >> 24 == 3)
            } else {
                None
            };
        }
    }

    pub(super) fn refill(&mut self, pc: u32, fetched: [u16; 2]) {
        debug_assert!(self.pending.get().is_none());
        debug_assert_eq!(pc & 1, 0);
        let mut lanes = Lanes::default();
        for (index, half) in fetched.into_iter().enumerate() {
            lanes.drive(
                pc + index as u32 * 2,
                AccessWidth::Halfword,
                u32::from(half),
            );
        }
        self.committed = Some(History { next_pc: pc, lanes });
    }

    pub(super) fn invalidate(&mut self) {
        debug_assert!(self.pending.get().is_none());
        self.committed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_lanes_stay_unknown_until_all_bytes_have_been_driven() {
        let mut bus = IwramBus::default();
        bus.begin(Some(0x0300_0100), Some(0x1234));
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0300_2002, AccessWidth::Byte, 0x56);
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0200_2003, AccessWidth::Byte, 0xff); // Other memory cannot fill a lane.
        assert_eq!(bus.snapshot(), None);
        bus.access(0x0300_2003, AccessWidth::Byte, 0x78);
        assert_eq!(bus.snapshot(), Some(0x7856_1234));
        bus.finish(true, true);
        bus.begin(Some(0x0300_0102), Some(0xabcd));
        assert_eq!(bus.snapshot(), Some(0xabcd_1234));
    }

    #[test]
    fn failed_transaction_discards_all_staged_accesses_not_only_prefetch() {
        let mut bus = IwramBus::default();
        bus.begin(Some(0x0300_0100), Some(0x1234));
        bus.access(0x0300_2000, AccessWidth::Word, 0x1122_3344);
        bus.finish(true, true);
        bus.begin(Some(0x0300_0102), Some(0xabcd));
        bus.access(0x0300_2000, AccessWidth::Word, 0xdead_beef);
        bus.access(0x0300_2001, AccessWidth::Byte, 0x55);
        assert_eq!(bus.snapshot(), Some(0xdead_55ef));
        bus.finish(false, false);
        bus.begin(Some(0x0300_0102), Some(0x5678));
        assert_eq!(bus.snapshot(), Some(0x5678_3344));
    }
}
