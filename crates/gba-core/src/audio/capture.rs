//! Optional fixed-rate digital capture, separate from hardware channel state.
use super::StereoLevel;
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
