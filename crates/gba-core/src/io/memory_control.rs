//! Internal memory control at `0x04000800`, repeated every 64 KiB of the I/O page.
//!
//! Follows mGBA 0.10.5 (`GBAIOWrite` case `REG_EXWAITCNT_HI` and `GBAAdjustEWRAMWaitstates`
//! in `src/gba/`): the low halfword ignores writes and reads its reset value `0x0020`; the high
//! halfword keeps bits 8-15 of what was written, and bits 8-11 (N) set the external work RAM
//! wait states to 15 - N. N = 15 (no wait states) is refused and keeps the previous timing, as
//! in mGBA; GBATEK reports that setting locks up hardware. Not hardware-verified.

use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};

/// The register's first address; mirrors repeat it every 64 KiB through the I/O page.
pub const MEMCNT: u32 = 0x0400_0800;
const LOW: u16 = 0x0020;
const RESET_HIGH: u16 = 0x0d00;
/// EWRAM wait states after reset: 15 - 0xd.
pub(crate) const RESET_EWRAM_WAIT: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryControl {
    high: u16,
    ewram_wait: u8,
}

impl Default for MemoryControl {
    fn default() -> Self {
        Self {
            high: RESET_HIGH,
            ewram_wait: RESET_EWRAM_WAIT,
        }
    }
}

impl MemoryControl {
    pub(crate) fn mapped(address: u32) -> bool {
        address & 0xff00_fffc == MEMCNT
    }

    pub(crate) fn read8(&self, address: u32) -> u8 {
        let half = if address & 2 == 0 { LOW } else { self.high };
        half.to_le_bytes()[(address & 1) as usize]
    }

    pub(crate) fn write8(&mut self, address: u32, value: u8) {
        if address & 3 != 3 {
            return; // The low halfword and bits 16-23 are not stored.
        }
        self.high = u16::from(value) << 8;
        let wait = 15 - (value & 15);
        if wait != 0 {
            self.ewram_wait = wait;
        }
    }

    pub(crate) fn ewram_wait(&self) -> u8 {
        self.ewram_wait
    }
}

impl Snapshot for MemoryControl {
    fn save(&self, writer: &mut Writer) {
        let Self { high, ewram_wait } = self;
        writer.u16(*high);
        writer.u8(*ewram_wait);
    }

    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let high = reader.u16()?;
        let ewram_wait = reader.u8()?;
        if high & 0xff != 0 || !(1..=15).contains(&ewram_wait) {
            return Err(SnapshotError::Corrupt("memory control"));
        }
        *self = Self { high, ewram_wait };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resets_to_0d000020_and_only_the_high_byte_is_writable() {
        let mut control = MemoryControl::default();
        let read = |control: &MemoryControl| {
            u32::from_le_bytes([0, 1, 2, 3].map(|offset| control.read8(MEMCNT + offset)))
        };
        assert_eq!(read(&control), 0x0d00_0020);
        for (offset, value) in [(0, 0xff), (1, 0xff), (2, 0xff), (3, 0xee)] {
            control.write8(MEMCNT + offset, value);
        }
        assert_eq!(read(&control), 0xee00_0020);
        assert_eq!(control.ewram_wait(), 1);
        control.write8(0x0401_0803, 0x0f); // A mirror; N = 15 keeps the previous waits.
        assert_eq!((read(&control), control.ewram_wait()), (0x0f00_0020, 1));
        assert!(MemoryControl::mapped(0x04ff_0802) && !MemoryControl::mapped(0x0400_0804));
    }
}
