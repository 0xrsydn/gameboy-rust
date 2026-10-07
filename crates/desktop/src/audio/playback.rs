//! Bounded host buffering and linear rate conversion. No device access here.
use gba_core::audio::{StereoLevel, AUDIO_SAMPLE_RATE};
use std::collections::VecDeque;

pub(super) const CAPACITY: usize = 8192;
const PREFILL: usize = 1024;

pub(super) struct Playback {
    queue: VecDeque<StereoLevel>,
    rate: u32,
    channels: usize,
    phase: u32,
    pair: Option<[StereoLevel; 2]>,
    previous: [f32; 2],
    filtered: [f32; 2],
    decay: f32,
    pub(super) submitted: u64,
    pub(super) nonzero_input: u64,
    pub(super) dropped: u64,
    pub(super) underruns: u64,
}
impl Playback {
    pub(super) fn new(rate: u32, channels: usize) -> Self {
        assert!((8000..=192000).contains(&rate) && (1..=32).contains(&channels));
        Self {
            queue: VecDeque::with_capacity(CAPACITY),
            rate,
            channels,
            phase: 0,
            pair: None,
            previous: [0.; 2],
            filtered: [0.; 2],
            decay: (-std::f32::consts::TAU * 20. / rate as f32).exp(),
            submitted: 0,
            nonzero_input: 0,
            dropped: 0,
            underruns: 0,
        }
    }
    pub(super) fn submit(&mut self, samples: &[StereoLevel]) {
        self.submitted = self.submitted.saturating_add(samples.len() as u64);
        self.nonzero_input = self.nonzero_input.saturating_add(
            samples
                .iter()
                .filter(|s| s.left != 0 || s.right != 0)
                .count() as u64,
        );
        for &sample in samples {
            if self.queue.len() == CAPACITY {
                self.dropped = self.dropped.saturating_add(1);
            } else {
                self.queue.push_back(sample);
            }
        }
    }
    pub(super) fn clear(&mut self) {
        self.queue.clear();
        self.reset();
    }
    fn reset(&mut self) {
        self.pair = None;
        self.phase = 0;
        self.previous = [0.; 2];
        self.filtered = [0.; 2];
    }
    fn next(&mut self) -> [f32; 2] {
        if self.pair.is_none() {
            if self.queue.len() < PREFILL {
                return [0.; 2];
            }
            self.pair = Some([
                self.queue.pop_front().unwrap(),
                self.queue.pop_front().unwrap(),
            ]);
        }
        let [a, b] = self.pair.unwrap();
        let fraction = self.phase as f32 / self.rate as f32;
        let mut output = [0.; 2];
        for (side, (a, b)) in [(a.left, b.left), (a.right, b.right)]
            .into_iter()
            .enumerate()
        {
            // Conservative 25% host gain and 20 Hz DC removal; not a hardware analog model.
            let input = (f32::from(a) + (f32::from(b) - f32::from(a)) * fraction) / 512. * 0.25;
            self.filtered[side] = input - self.previous[side] + self.decay * self.filtered[side];
            self.previous[side] = input;
            output[side] = self.filtered[side].clamp(-1., 1.);
        }
        self.phase += AUDIO_SAMPLE_RATE;
        while self.phase >= self.rate {
            self.phase -= self.rate;
            if let Some(next) = self.queue.pop_front() {
                let old = self.pair.unwrap()[1];
                self.pair = Some([old, next]);
            } else {
                self.underruns = self.underruns.saturating_add(1);
                self.reset();
                break;
            }
        }
        output
    }
    pub(super) fn render<T: Copy>(
        &mut self,
        output: &mut [T],
        convert: impl Fn(f32) -> T,
    ) -> usize {
        output.fill(convert(0.));
        let mut nonzero = 0;
        for frame in output.chunks_exact_mut(self.channels) {
            let [left, right] = self.next();
            if self.channels == 1 {
                frame[0] = convert((left + right) * 0.5);
            } else {
                frame[0] = convert(left);
                frame[1] = convert(right);
            }
            nonzero += usize::from(left != 0. || right != 0.);
        }
        nonzero
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(left: i16, right: i16) -> StereoLevel {
        StereoLevel { left, right }
    }
    #[test]
    fn prefill_silence_stereo_mapping_extra_channels_and_clear() {
        let mut p = Playback::new(48000, 4);
        let mut out = [9.; 16];
        p.render(&mut out, |x| x);
        assert_eq!(out, [0.; 16]);
        p.submit(&vec![sample(256, -256); PREFILL]);
        p.render(&mut out, |x| x);
        assert_eq!(&out[..4], &[0.125, -0.125, 0., 0.]);
        assert!(out[4] > 0. && out[4] < 0.125);
        p.clear();
        p.render(&mut out, |x| x);
        assert_eq!(out, [0.; 16]);
    }
    #[test]
    fn split_callbacks_keep_phase_and_rate_conversion_consumption() {
        for rate in [8000, 32768, 44100, 48000, 192000] {
            let mut a = Playback::new(rate, 2);
            let mut b = Playback::new(rate, 2);
            let data: Vec<_> = (0..4096)
                .map(|i| sample((i % 1024) as i16 - 512, 0))
                .collect();
            a.submit(&data);
            b.submit(&data);
            let mut one = vec![0.; 2000];
            let mut split = one.clone();
            a.render(&mut one, |x| x);
            for part in split.chunks_mut(14) {
                b.render(part, |x| x);
            }
            assert_eq!(one, split);
            assert_eq!(a.phase, b.phase);
            assert_eq!(a.queue, b.queue);
            if rate >= 32768 {
                assert_eq!(
                    a.queue.len(),
                    4096 - 2 - (1000 * AUDIO_SAMPLE_RATE / rate) as usize
                );
            }
        }
    }
    #[test]
    fn overflow_underflow_and_recovery_are_bounded() {
        let mut p = Playback::new(32768, 1);
        p.submit(&vec![sample(100, -100); CAPACITY + 7]);
        assert_eq!(p.dropped, 7);
        let mut out = vec![1.; CAPACITY + 100];
        p.render(&mut out, |x| x);
        assert!(out.iter().all(|x| *x == 0.));
        assert_eq!(p.underruns, 1);
        p.submit(&vec![sample(256, 256); PREFILL]);
        p.render(&mut out[..1], |x| x);
        assert_eq!(out[0], 0.125);
    }
    #[test]
    fn dc_offset_decays_without_out_of_range_or_nonfinite_samples() {
        let mut p = Playback::new(32768, 2);
        p.submit(&vec![sample(-512, 511); CAPACITY]);
        let mut out = vec![0.; 12000];
        p.render(&mut out, |x| x);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 0.25));
        assert!(out[11999].abs() < 0.000001);
    }
}
