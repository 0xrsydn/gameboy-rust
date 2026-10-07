//! macOS CoreAudio through CPAL. No blocking or allocation in the data callback.
use super::playback::Playback;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use gba_core::audio::StereoLevel;
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

struct Shared {
    playback: Mutex<Playback>,
    failed: AtomicBool,
    callbacks: AtomicU64,
    nonzero: AtomicU64,
    contention: AtomicU64,
}
pub struct AudioOutput {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    rate: u32,
    channels: u16,
}
impl AudioOutput {
    pub fn new() -> io::Result<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                io::Error::other("no default audio output device; omit --audio to run muted")
            })?;
        let config = device.default_output_config().map_err(io::Error::other)?;
        let rate = config.sample_rate().0;
        let channels = config.channels();
        if !(8000..=192000).contains(&rate) || !(1..=32).contains(&channels) {
            return Err(io::Error::other(
                "unsupported audio rate/channel count; omit --audio to run muted",
            ));
        }
        let shared = Arc::new(Shared {
            playback: Mutex::new(Playback::new(rate, usize::from(channels))),
            failed: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            nonzero: AtomicU64::new(0),
            contention: AtomicU64::new(0),
        });
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), shared.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), shared.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), shared.clone()),
            format => {
                return Err(io::Error::other(format!(
                    "unsupported output sample format {format}; omit --audio to run muted"
                )))
            }
        }?;
        stream.play().map_err(io::Error::other)?;
        Ok(Self {
            _stream: stream,
            shared,
            rate,
            channels,
        })
    }
    fn check(&self) -> io::Result<()> {
        if self.shared.failed.load(Ordering::Relaxed) {
            Err(io::Error::other(
                "audio output stream failed; restart without --audio to run muted",
            ))
        } else {
            Ok(())
        }
    }
    pub fn submit(&self, samples: &[StereoLevel]) -> io::Result<()> {
        self.check()?;
        self.shared
            .playback
            .lock()
            .map_err(|_| io::Error::other("audio queue poisoned"))?
            .submit(samples);
        Ok(())
    }
    pub fn clear(&self) -> io::Result<()> {
        self.check()?;
        self.shared
            .playback
            .lock()
            .map_err(|_| io::Error::other("audio queue poisoned"))?
            .clear();
        Ok(())
    }
    pub fn description(&self) -> String {
        format!(
            "CoreAudio: {} Hz, {} channels; linear conversion, 25% gain, DC removal",
            self.rate, self.channels
        )
    }
    pub fn report(&self, writer: &mut impl Write) -> io::Result<()> {
        let p = self
            .shared
            .playback
            .lock()
            .map_err(|_| io::Error::other("audio queue poisoned"))?;
        writeln!(writer,"Audio callbacks: {}; nonzero output frames: {}; underruns: {}; dropped input frames: {}; callback lock misses: {}", self.shared.callbacks.load(Ordering::Relaxed),self.shared.nonzero.load(Ordering::Relaxed),p.underruns,p.dropped,self.shared.contention.load(Ordering::Relaxed))?;
        writeln!(
            writer,
            "Audio submitted frames: {}; nonzero input frames: {}",
            p.submitted, p.nonzero_input
        )?;
        self.check()
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> io::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let errors = shared.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                data.fill(T::from_sample(0.));
                shared.callbacks.fetch_add(1, Ordering::Relaxed);
                match shared.playback.try_lock() {
                    Ok(mut p) => {
                        let nonzero = p.render(data, T::from_sample);
                        shared.nonzero.fetch_add(nonzero as u64, Ordering::Relaxed);
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {
                        shared.contention.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        shared.failed.store(true, Ordering::Relaxed);
                    }
                }
            },
            move |_| {
                errors.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a native default audio device; plays a quiet original test tone"]
    fn native_audio_output_consumes_original_samples() {
        let output = AudioOutput::new().unwrap();
        // Exercise the actual core capture path without depending on window focus.
        let mut memory = gba_core::memory::Memory::new(vec![]).unwrap();
        for (address, value) in [
            (0x04000084, 0x80),
            (0x04000088, 0x200),
            (0x04000082, 2),
            (0x04000080, 0x1177),
            (0x04000062, 0x3080),
            (0x04000064, 0x86d6),
        ] {
            memory.write16(address, value).unwrap();
        }
        memory.set_audio_capture(true);
        memory.advance_cycles(4096 * 512);
        let mut samples = vec![StereoLevel::default(); 4096];
        assert_eq!(memory.drain_audio_samples(&mut samples), 4096);
        assert_eq!(memory.audio_dropped_samples(), 0);
        assert!(samples.iter().any(|s| s.left > 0));
        assert!(samples.iter().any(|s| s.left < 0));
        output.submit(&samples).unwrap();
        let start = std::time::Instant::now();
        while output.shared.nonzero.load(Ordering::Relaxed) == 0
            && start.elapsed() < std::time::Duration::from_secs(3)
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
            output.check().unwrap();
        }
        output.report(&mut std::io::stdout()).unwrap();
        assert!(output.shared.nonzero.load(Ordering::Relaxed) > 0);
        output.clear().unwrap();
    }
}
