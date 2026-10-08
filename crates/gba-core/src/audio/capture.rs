//! Optional fixed-rate digital capture, separate from hardware channel state.
use super::StereoLevel;
use crate::snapshot::{Reader, Snapshot, SnapshotError, Writer};
use std::collections::VecDeque;

/// Nominal stereo capture rate. This is not the GBA PWM/resolution model.
pub const AUDIO_SAMPLE_RATE: u32 = 32768;
/// Maximum committed stereo frames retained before the caller drains them.
pub const AUDIO_QUEUE_CAPACITY: usize = 4096;
const PERIOD: u32 = 512;
pub(crate) const STEP_SAMPLES: usize = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SampleClock(Option<u16>);

impl SampleClock {
    /// Advance devices at every retained sample boundary. Once capacity is
    /// exhausted, advance the remainder in bulk and count discarded samples.
    pub(crate) fn advance(
        &mut self,
        mut cycles: u32,
        mut capacity: usize,
        mut advance: impl FnMut(u32, bool),
    ) -> u64 {
        let Some(mut phase) = self.0 else {
            advance(cycles, false);
            return 0;
        };
        while cycles != 0 && capacity != 0 {
            let count = cycles.min(PERIOD - u32::from(phase));
            phase += count as u16;
            cycles -= count;
            let sample = u32::from(phase) == PERIOD;
            if sample {
                phase = 0;
                capacity -= 1;
            }
            advance(count, sample);
        }
        let discarded = (u64::from(phase) + u64::from(cycles)) / u64::from(PERIOD);
        if cycles != 0 {
            advance(cycles, false);
        }
        self.0 = Some(((u64::from(phase) + u64::from(cycles)) % u64::from(PERIOD)) as u16);
        discarded
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SampleBatch {
    pub(crate) samples: [StereoLevel; STEP_SAMPLES],
    pub(crate) len: usize,
    pub(crate) dropped: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AudioCapture {
    pub(crate) clock: SampleClock,
    queue: VecDeque<StereoLevel>,
    pub(crate) dropped: u64,
}

impl AudioCapture {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.clock.0.is_some() {
            return;
        }
        self.clock = SampleClock(enabled.then_some(0));
        self.queue = if enabled {
            VecDeque::with_capacity(AUDIO_QUEUE_CAPACITY)
        } else {
            VecDeque::new()
        };
        self.dropped = 0;
    }

    pub(crate) fn capacity(&self) -> usize {
        AUDIO_QUEUE_CAPACITY - self.queue.len()
    }

    pub(crate) fn push(&mut self, sample: StereoLevel) {
        if self.queue.len() < AUDIO_QUEUE_CAPACITY {
            self.queue.push_back(sample);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(crate) fn commit(&mut self, clock: SampleClock, batch: SampleBatch) {
        self.clock = clock;
        self.dropped = self.dropped.saturating_add(batch.dropped);
        for sample in &batch.samples[..batch.len] {
            self.push(*sample);
        }
    }

    pub(crate) fn drain(&mut self, output: &mut [StereoLevel]) -> usize {
        let count = output.len().min(self.queue.len());
        for slot in &mut output[..count] {
            *slot = self.queue.pop_front().unwrap();
        }
        count
    }
}

impl Snapshot for AudioCapture {
    /// The sample-clock phase continues the emulated timeline, so it is state.
    /// The queue and drop count are host-facing output and are not.
    fn save(&self, writer: &mut Writer) {
        let Self {
            clock,
            queue: _queue,     // Host-facing output, not hardware state.
            dropped: _dropped, // Host-facing counter, not hardware state.
        } = self;
        writer.tag(b"ACAP");
        writer.option(clock.0.as_ref(), |phase, writer| writer.u16(*phase));
    }

    /// Whether capture runs is host configuration and is kept. A running capture
    /// resumes the saved phase, or starts at phase zero if none was running.
    fn load(&mut self, reader: &mut Reader<'_>) -> Result<(), SnapshotError> {
        let Self {
            clock,
            queue,
            dropped,
        } = self;
        reader.tag(b"ACAP")?;
        let saved = reader.option(Reader::u16)?;
        if saved.is_some_and(|phase| u32::from(phase) >= PERIOD) {
            return Err(SnapshotError::Corrupt("audio capture phase"));
        }
        if clock.0.is_some() {
            clock.0 = Some(saved.unwrap_or(0));
        }
        queue.clear();
        *dropped = 0;
        Ok(())
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn encoded(capture: &AudioCapture) -> Vec<u8> {
        let mut writer = Writer::new();
        capture.save(&mut writer);
        writer.finish().unwrap()
    }

    #[test]
    fn snapshot_restores_phase_and_clears_host_output() {
        let mut source = AudioCapture::default();
        source.set_enabled(true);
        source.clock.0 = Some(317);
        source.push(StereoLevel {
            left: 12,
            right: -9,
        });
        source.dropped = 7;
        let bytes = encoded(&source);

        let mut loaded = AudioCapture::default();
        loaded.set_enabled(true);
        loaded.push(StereoLevel { left: 1, right: 2 });
        loaded.dropped = 4;
        loaded.load(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(loaded.clock.0, Some(317));
        assert!(loaded.queue.is_empty());
        assert_eq!(loaded.dropped, 0);
        for end in 0..bytes.len() {
            assert!(loaded.load(&mut Reader::new(&bytes[..end])).is_err());
        }

        let mut corrupt = bytes.clone();
        corrupt[0] ^= 1;
        assert_eq!(
            loaded.load(&mut Reader::new(&corrupt)),
            Err(SnapshotError::Corrupt("section tag"))
        );
        let mut out_of_range = bytes;
        out_of_range[5..7].copy_from_slice(&512u16.to_le_bytes());
        assert_eq!(
            loaded.load(&mut Reader::new(&out_of_range)),
            Err(SnapshotError::Corrupt("audio capture phase"))
        );
    }

    #[test]
    fn capture_configuration_is_kept_not_restored() {
        let mut running = AudioCapture::default();
        running.set_enabled(true);
        running.clock.0 = Some(100);
        let running_bytes = encoded(&running);
        let idle_bytes = encoded(&AudioCapture::default());

        // A state saved while capture ran does not enable capture on an idle target.
        let mut idle = AudioCapture::default();
        idle.load(&mut Reader::new(&running_bytes)).unwrap();
        assert_eq!(idle.clock.0, None);
        // A state saved while idle starts a running capture at phase zero.
        let mut target = AudioCapture::default();
        target.set_enabled(true);
        target.clock.0 = Some(200);
        target.load(&mut Reader::new(&idle_bytes)).unwrap();
        assert_eq!(target.clock.0, Some(0));
    }
}
