//! Staged timer and audio clocks for one instruction, IRQ entry, or DMA unit.
//! Other devices remain on the instruction-boundary scheduler.
use super::{Io, Timer, IF, TIMER_BASE};

pub(super) const TIMER_IRQ_MASK: u16 = 0x78;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerStep {
    pub(crate) audio: crate::audio::Audio,
    pub(super) timers: [Timer; 4],
    pub(super) phase: u16,
    pub(super) pending: u16,
    elapsed: u32,
}

impl TimerStep {
    pub(super) fn new(io: &Io) -> Self {
        Self {
            audio: io.audio,
            timers: io.timers,
            phase: io.timer_phase,
            pending: io.pending & TIMER_IRQ_MASK,
            elapsed: 0,
        }
    }

    pub(crate) fn advance_to(&mut self, elapsed: u32) {
        assert!(elapsed >= self.elapsed, "timer step time must be monotonic");
        self.pending |= Io::advance_timer_bank(
            &mut self.timers,
            &mut self.phase,
            &mut self.audio,
            elapsed - self.elapsed,
        );
        self.elapsed = elapsed;
    }

    /// All lanes of a bus access observe the same staged time.
    pub(crate) fn read8(&self, address: u32, committed: u8) -> Option<u8> {
        if crate::audio::Audio::mapped(address) {
            return self.audio.read8(address);
        }
        match address {
            TIMER_BASE..=0x0400_010f => {
                Some(self.timers[((address - TIMER_BASE) / 4) as usize].read8(address))
            }
            IF => Some((committed & !(TIMER_IRQ_MASK as u8)) | self.pending as u8),
            _ => None,
        }
    }

    /// Return true only when the staged state owns the entire register byte.
    pub(crate) fn write8(&mut self, address: u32, value: u8) -> bool {
        match address {
            TIMER_BASE..=0x0400_010f => {
                let index = ((address - TIMER_BASE) / 4) as usize;
                self.timers[index].write8(address, value, index);
                true
            }
            IF => {
                self.pending &= !u16::from(value);
                false // The normal I/O path must also acknowledge non-timer sources.
            }
            _ => false,
        }
    }
}
