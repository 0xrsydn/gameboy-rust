//! Staged timer, audio, and serial clocks for one instruction, IRQ entry, or DMA unit.
//! Other devices remain on the instruction-boundary scheduler.
use super::{Io, Timer, IF, TIMER_BASE, TIMER_START_DELAY};
use crate::audio::capture::{SampleBatch, SampleClock, STEP_SAMPLES};

pub(super) const STAGED_IRQ_MASK: u16 = 0xf8;

/// Longest run of deferred device time between settles, so the deferred counter
/// stays bounded when no device has an event scheduled.
const QUIET_CAP: u32 = 1 << 24;

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
    /// Advance from `from` cycles into the step by `cycles`. Record the exact cycle at
    /// which the first new enabled request occurred, so the date does not depend on how
    /// the step is split into batches.
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
        let mut raise = |bits: u16, at: &dyn Fn(u16) -> u32| {
            let new = bits & enable & !*pending;
            if new != 0 {
                // A later batch cannot date a request before an earlier one did; two
                // devices in the same batch can, so keep the earlier cycle.
                let at = at(new);
                *raised_at = Some(raised_at.map_or(at, |first| first.min(at)));
            }
            *pending |= bits;
        };
        // An unclocked serial port has no event and its advance does nothing.
        if let Some(completion) = serial.next_event_cycles() {
            raise(serial.advance(cycles), &|_| from + completion.min(cycles));
        }
        let mut base = from;
        let capacity = STEP_SAMPLES - samples.len;
        let dropped = sample_clock.advance(cycles, capacity, |count, sample| {
            let before = (*timers, *phase);
            let bits = Io::advance_timer_bank(timers, phase, audio, count);
            raise(bits, &|new| {
                base + Io::first_timer_irq(&before.0, before.1, count, new)
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
        let mut step = Self {
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
        };
        // Deferred time ends before the next device event, so applying it here raises
        // nothing and records no sample; the step then starts from the settled state.
        if io.deferred != 0 {
            step.advance_to(io.deferred);
            debug_assert!(step.raised_at.is_none() && step.samples.len == 0);
            debug_assert_eq!(step.pending, io.pending & STAGED_IRQ_MASK);
            step.elapsed = 0;
        }
        step
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

    /// All lanes of a bus access observe the same staged time. `committed` supplies the
    /// non-staged `IF` bits and is evaluated only for `IF`.
    pub(crate) fn read8(&self, address: u32, committed: impl FnOnce() -> u8) -> Option<u8> {
        if crate::audio::Audio::mapped(address) {
            return self.audio.read8(address);
        }
        match address {
            TIMER_BASE..=0x0400_010f => {
                Some(self.timers[((address - TIMER_BASE) / 4) as usize].read8(address))
            }
            IF => Some((committed() & !(STAGED_IRQ_MASK as u8)) | self.pending as u8),
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
    ///
    /// A step that ends before the next device event (timer overflow, serial completion,
    /// or capture sample) only adds its cycles to `deferred`. Every counter these devices
    /// keep advances additively, so applying the deferred cycles later in one batch gives
    /// the same state. Anything that changes or observes them first settles or stages.
    #[inline]
    pub(crate) fn advance_staged_devices(&mut self, cycles: u32) {
        if cycles < self.quiet_left {
            self.quiet_left -= cycles;
            self.deferred += cycles;
            self.update_irq_line(Some(cycles));
            return;
        }
        self.settle_devices();
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
        self.quiet_left = self.quiet_cycles();
    }

    /// Apply deferred device time. Callers that change or observe the timers, sound,
    /// serial port, or capture clock outside a staged step settle first.
    pub(crate) fn settle_devices(&mut self) {
        self.quiet_left = 0;
        if self.deferred == 0 {
            return;
        }
        let step = TimerStep::new(self);
        self.deferred = 0;
        self.audio = step.audio;
        self.audio_capture.commit(step.sample_clock, step.samples);
        self.serial = step.serial;
        self.timers = step.timers;
        self.timer_phase = step.phase;
    }

    /// Cycles from now to the first device event, capped. A step shorter than this
    /// raises no request, triggers no sound DMA, and records no sample.
    pub(super) fn quiet_cycles(&self) -> u32 {
        // Stop and reload edges act in the cycle after a write; keep them per step.
        if self
            .timers
            .iter()
            .any(|timer| timer.stopping.is_some() || timer.stale_reload.is_some())
        {
            return 0;
        }
        self.timers
            .iter()
            .filter_map(|timer| timer.next_overflow(self.timer_phase))
            .chain(self.serial.next_event_cycles())
            .chain(self.audio_capture.clock.until_sample())
            .fold(QUIET_CAP, u32::min)
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

    fn timer(io: &mut Io, index: u32, reload: u16, control: u8) {
        let base = TIMER_BASE + index * 4;
        for (offset, byte) in reload.to_le_bytes().into_iter().enumerate() {
            io.write8(base + offset as u32, byte);
        }
        io.write8(base + 2, control);
    }

    fn wait_after(io: &Io, parts: &[u32]) -> Option<u32> {
        let mut io = io.clone();
        let mut step = io.timer_step();
        let mut elapsed = 0;
        for part in parts {
            elapsed += part;
            step.advance_to(elapsed);
        }
        io.commit_timer_step(step);
        io.irq_wait
    }

    #[test]
    fn a_masked_timer_does_not_date_another_timers_request() {
        let mut io = Io::default();
        timer(&mut io, 0, 0xfffc, 0xc0); // Overflows at cycle 4; its request is masked.
        timer(&mut io, 2, 0xfff0, 0xc0); // Overflows at cycle 16.
        io.enable = 1 << 5;
        for parts in [&[20][..], &[6, 14], &[3, 3, 14], &[16, 4]] {
            assert_eq!(
                wait_after(&io, parts),
                Some(16 + crate::io::IRQ_DELAY),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn a_cascaded_overflow_is_dated_at_the_overflow_that_caused_it() {
        let mut io = Io::default();
        timer(&mut io, 0, 0xfffc, 0x80); // Overflows every 4 cycles.
        timer(&mut io, 1, 0xfffe, 0xc4); // Counts those; overflows at cycle 8.
        io.enable = 1 << 4;
        for parts in [&[9][..], &[6, 3], &[8, 1], &[2, 2, 5]] {
            assert_eq!(
                wait_after(&io, parts),
                Some(8 + crate::io::IRQ_DELAY),
                "{parts:?}"
            );
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
                // Deferred time carries across rounds. These direct field writes bypass
                // the register paths, which settle it themselves.
                if round % 16 == 0 {
                    io.settle_devices();
                    scramble(&mut io, &mut random);
                }
                // Requests must be dated the same way however the step is split, so
                // enable some, acknowledge some, and let the line drop between rounds.
                io.enable = random.next() as u16 & 0x00f8;
                io.pending &= random.next() as u16;
                io.irq_wait = None;
                io.update_irq_line(None);
                if round % 5 == 0 {
                    // CPU-style writes leave stop, reload, and start edges pending.
                    let index = random.next() as usize % 4;
                    let address = TIMER_BASE + index as u32 * 4 + (random.next() & 2);
                    let value = random.next() as u8;
                    io.settle_devices();
                    io.timers[index].write8(address, value, index, TIMER_START_DELAY);
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
                let mut settled = io.clone();
                settled.settle_devices();
                assert_eq!(settled, whole, "seed {seed} round {round} parts {parts:?}");
                let mut drained = [Default::default(); 64];
                while io.audio_capture.drain(&mut drained) != 0 {}
            }
        }
    }
}
