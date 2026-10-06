//! Host-only clock adapter. UTC is sampled once; later changes use monotonic elapsed seconds.
use gba_core::{
    cartridge::{RtcDateTime, RtcError},
    memory::Memory,
};
use std::{
    io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const UNIX_2000: u64 = 946_684_800;

pub(crate) struct RtcHostClock {
    started: Instant,
    supplied: u64,
}

impl RtcHostClock {
    pub(crate) fn new(memory: &mut Memory) -> io::Result<Self> {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs();
        Self::from_unix_seconds(memory, unix).map_err(io::Error::other)
    }

    fn from_unix_seconds(memory: &mut Memory, unix: u64) -> Result<Self, RtcError> {
        let seconds = unix
            .checked_sub(UNIX_2000)
            .ok_or(RtcError::InvalidDateTime)?;
        memory.set_rtc_datetime(RtcDateTime::from_seconds_since_2000(seconds)?)?;
        Ok(Self {
            started: Instant::now(),
            supplied: 0,
        })
    }

    pub(crate) fn sync(&mut self, memory: &mut Memory) -> Result<(), RtcError> {
        self.supply(memory, self.started.elapsed())
    }

    fn supply(&mut self, memory: &mut Memory, elapsed: Duration) -> Result<(), RtcError> {
        let seconds = elapsed.as_secs();
        // Ignore a backwards test/source sample without moving the high-water mark.
        if seconds > self.supplied {
            memory.advance_rtc_seconds(seconds - self.supplied)?;
            self.supplied = seconds;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_core::{cartridge::CartridgeHardware, io::HALTCNT};
    fn memory() -> Memory {
        let mut memory = Memory::new(vec![0; 4]).unwrap();
        memory.set_cartridge_hardware(CartridgeHardware::Rtc);
        memory
    }

    #[test]
    fn epoch_seed_is_utc_and_range_checked_without_changing_state_on_error() {
        let mut memory = memory();
        RtcHostClock::from_unix_seconds(&mut memory, UNIX_2000).unwrap();
        assert_eq!(
            memory.rtc_datetime().unwrap().components(),
            [0, 1, 1, 6, 0, 0, 0]
        );
        for invalid in [UNIX_2000 - 1, UNIX_2000 + 3_155_760_000, u64::MAX] {
            assert!(RtcHostClock::from_unix_seconds(&mut memory, invalid).is_err());
            assert_eq!(
                memory.rtc_datetime().unwrap().components(),
                [0, 1, 1, 6, 0, 0, 0]
            );
        }
    }

    #[test]
    fn elapsed_seconds_are_supplied_once_preserve_guest_time_and_continue_in_stop() {
        let mut memory = memory();
        let mut clock = RtcHostClock::from_unix_seconds(&mut memory, UNIX_2000).unwrap();
        for ms in [999, 1000, 1000, 700, 1999, 2000] {
            clock
                .supply(&mut memory, Duration::from_millis(ms))
                .unwrap();
        }
        assert_eq!(
            memory.rtc_datetime().unwrap().components(),
            [0, 1, 1, 6, 0, 0, 2]
        );
        memory
            .set_rtc_datetime(RtcDateTime::new([24, 2, 29, 3, 23, 59, 59]).unwrap())
            .unwrap();
        memory.write8(HALTCNT, 128).unwrap();
        clock.supply(&mut memory, Duration::from_secs(3)).unwrap();
        assert_eq!(
            memory.rtc_datetime().unwrap().components(),
            [24, 3, 1, 4, 0, 0, 0]
        );
        assert!(memory.stopped());
        assert_eq!(memory.cycles(), 0);
    }

    #[test]
    fn failed_tick_does_not_lose_elapsed_seconds() {
        let mut memory = memory();
        let mut clock = RtcHostClock::from_unix_seconds(&mut memory, UNIX_2000).unwrap();
        memory.set_cartridge_hardware(CartridgeHardware::None);
        assert_eq!(
            clock.supply(&mut memory, Duration::from_secs(5)),
            Err(RtcError::NotAttached)
        );
        memory.set_cartridge_hardware(CartridgeHardware::Rtc);
        clock.supply(&mut memory, Duration::from_secs(5)).unwrap();
        assert_eq!(
            memory.rtc_datetime().unwrap().components(),
            [0, 1, 1, 0, 0, 0, 5]
        );
    }
}
