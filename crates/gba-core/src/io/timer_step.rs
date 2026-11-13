//! Staged timer, audio, and serial clocks for one instruction, IRQ entry, or DMA unit.
//! Other devices remain on the instruction-boundary scheduler.
use super::{Io, Timer, IF, TIMER_BASE, TIMER_START_DELAY};
use crate::audio::capture::{SampleBatch, SampleClock, STEP_SAMPLES};

pub(super) const STAGED_IRQ_MASK: u16 = 0xf8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerStep {
    pub(crate) audio: crate::audio::Audio,
    pub(crate) sample_clock: crate::audio::capture::SampleClock,
    pub(crate) samples: crate::audio::capture::SampleBatch,
    pub(crate) serial: super::Serial,
    pub(super) timers: [Timer; 4],
    pub(super) phase: u16,
    pub(super) pending: u16,
    /// `IE` when the shadow was staged, to date the first request it raises.
    enable: u16,
    raised_at: Option<u32>,
    elapsed: u32,
}

/// The clocks one step stages, borrowed either from a `TimerStep` or from `Io` itself.
struct Staged<'a> {
    audio: &'a mut crate::audio::Audio,
    sample_clock: &'a mut SampleClock,
    samples: &'a mut SampleBatch,
    serial: &'a mut super::Serial,
    timers: &'a mut [Timer; 4],
    phase: &'a mut u16,
    pending: &'a mut u16,
    enable: u16,
    raised_at: &'a mut Option<u32>,
}

impl Staged<'_> {
    /// Advance from `from` cycles into the step by `cycles`. Record when the first new
    /// enabled request occurred: serial completion and prescaled timer overflows at their
    /// exact cycle, a cascaded overflow at the end of its batch.
    #[inline]
    fn advance(self, from: u32, cycles: u32) {
        let Self {
            audio,
            sample_clock,
            samples,
            serial,
            timers,
            phase,
            pending,
            enable,
            raised_at,
        } = self;
        // Dating a request needs the timers as they were before the batch. Requests are
        // rare, so keep a copy and compute the overflow cycle only when one is raised.
        let mut raise = |bits: u16, at: &dyn Fn() -> u32| {
            if bits & enable & !*pending != 0 && raised_at.is_none() {
                *raised_at = Some(at());
            }
            *pending |= bits;
        };
        // An unclocked serial port has no event and its advance does nothing.
        if let Some(completion) = serial.next_event_cycles() {
            raise(serial.advance(cycles), &|| from + completion.min(cycles));
        }
        let mut base = from;
        let capacity = STEP_SAMPLES - samples.len;
        let dropped = sample_clock.advance(cycles, capacity, |count, sample| {
            let before = (*timers, *phase);
            let bits = Io::advance_timer_bank(timers, phase, audio, count);
            raise(bits, &|| {
                base + Io::first_timer_irq(&before.0, before.1, count).unwrap_or(count)
            });
            base += count;
            if sample {
                samples.samples[samples.len] = audio.level();
                samples.len += 1;
            }
        });
        samples.dropped = samples.dropped.saturating_add(dropped);
    }
}

impl TimerStep {
    pub(super) fn new(io: &Io) -> Self {
        Self {
            audio: io.audio,
            sample_clock: io.audio_capture.clock,
            samples: Default::default(),
            serial: io.serial,
            timers: io.timers,
            phase: io.timer_phase,
            pending: io.pending & STAGED_IRQ_MASK,
            enable: io.enable,
            raised_at: None,
            elapsed: 0,
        }
    }

    /// Addresses whose reads and writes go through the staged devices.
    pub(crate) fn owns(address: u32) -> bool {
        crate::audio::Audio::mapped(address)
            || super::Serial::mapped(address)
            || matches!(address, TIMER_BASE..=0x0400_010f | IF)
    }

    /// Cycles into the step at which this shadow first raised an enabled request, or the
    /// time it has reached if it raised none.
    pub(super) fn raised_at(&self) -> u32 {
        self.raised_at.unwrap_or(self.elapsed)
    }

    pub(crate) fn advance_to(&mut self, elapsed: u32) {
        assert!(elapsed >= self.elapsed, "timer step time must be monotonic");
        if elapsed == self.elapsed {
            return; // Several lanes of one access observe the same staged time.
        }
        Staged {
            audio: &mut self.audio,
            sample_clock: &mut self.sample_clock,
            samples: &mut self.samples,
            serial: &mut self.serial,
            timers: &mut self.timers,
            phase: &mut self.phase,
            pending: &mut self.pending,
            enable: self.enable,
            raised_at: &mut self.raised_at,
        }
        .advance(self.elapsed, elapsed - self.elapsed);
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
            IF => Some((committed & !(STAGED_IRQ_MASK as u8)) | self.pending as u8),
            _ => None,
        }
    }

    /// Return true only when the staged state owns the entire register byte.
    pub(crate) fn write8(&mut self, address: u32, value: u8) -> bool {
        match address {
            TIMER_BASE..=0x0400_010f => {
                let index = ((address - TIMER_BASE) / 4) as usize;
                self.timers[index].write8(address, value, index, TIMER_START_DELAY);
                true
            }
            IF => {
                self.pending &= !u16::from(value);
                false // The normal I/O path must also acknowledge non-staged sources.
            }
            _ => false,
        }
    }
}

impl Io {
    /// Advance the staged devices in place for a step that never observed them.
    /// This must equal staging a `TimerStep`, advancing it once, and committing it.
    pub(crate) fn advance_staged_devices(&mut self, cycles: u32) {
        let mut samples = SampleBatch::default();
        let mut sample_clock = self.audio_capture.clock;
        let mut raised_at = None;
        Staged {
            audio: &mut self.audio,
            sample_clock: &mut sample_clock,
            samples: &mut samples,
            serial: &mut self.serial,
            timers: &mut self.timers,
            phase: &mut self.timer_phase,
            pending: &mut self.pending,
            enable: self.enable,
            raised_at: &mut raised_at,
        }
        .advance(0, cycles);
        self.audio_capture.commit(sample_clock, samples);
        self.dma.trigger_sound(self.audio.take_requests());
        self.update_irq_line(Some(raised_at.unwrap_or(cycles)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{Audio, SOUNDCNT_X, SOUND_START};

    /// Deterministic pseudo-random values; the core takes no external crates.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as u32
        }
    }

    /// Reprogram timers, sound, and serial with arbitrary accepted register values.
    fn scramble(io: &mut Io, random: &mut Lcg) {
        for index in 0..4 {
            let base = TIMER_BASE + index * 4;
            io.write8(base, random.next() as u8);
            io.write8(base + 1, 0xf0 | random.next() as u8); // Overflow often.
            io.write8(base + 2, random.next() as u8);
        }
        io.audio.write(SOUNDCNT_X, &[0x80]);
        for address in SOUND_START..0x0400_00a8 {
            let value = random.next() as u8;
            if address != SOUNDCNT_X
                && Audio::mapped(address)
                && io.audio.unsupported(address, value).is_none()
            {
                io.audio.write(address, &[value]);
            }
        }
        let control = [0x81_u16, 0x83, 0x1081][random.next() as usize % 3].to_le_bytes();
        if io.serial.validate(0x0400_0128, &control).is_none() {
            io.serial.write(0x0400_0128, &control);
        }
    }

    fn cycles(random: &mut Lcg) -> u32 {
        match random.next() % 8 {
            0 => 0,
            1 => random.next() % 6000, // Longer than one step's sample batch.
            _ => random.next() % 40,
        }
    }

    /// `Memory` relies on this: a step that never observes the staged devices
    /// advances them once in place, and a shadow created mid-step catches up in
    /// one jump. Both must match the access-by-access staging they replace.
    #[test]
    fn staged_clocks_are_additive_and_match_the_in_place_advance() {
        for seed in 0..24 {
            let mut random = Lcg(seed);
            let mut io = Io::default();
            io.audio_capture.set_enabled(seed % 2 == 0);
            for round in 0..400 {
                if round % 16 == 0 {
                    scramble(&mut io, &mut random);
                }
                let parts = [
                    cycles(&mut random),
                    cycles(&mut random),
                    cycles(&mut random),
                ];
                let total: u32 = parts.iter().sum();

                let mut chunked = io.clone();
                let mut step = chunked.timer_step();
                let mut elapsed = 0;
                for part in parts {
                    elapsed += part;
                    step.advance_to(elapsed);
                }
                chunked.commit_timer_step(step);

                let mut whole = io.clone();
                let mut step = whole.timer_step();
                step.advance_to(total);
                whole.commit_timer_step(step);

                io.advance_staged_devices(total);
                assert_eq!(chunked, whole, "seed {seed} round {round} parts {parts:?}");
                assert_eq!(io, whole, "seed {seed} round {round} parts {parts:?}");
                let mut drained = [Default::default(); 64];
                while io.audio_capture.drain(&mut drained) != 0 {}
            }
        }
    }
}
