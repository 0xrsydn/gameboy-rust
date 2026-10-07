//! Main-thread ROM presentation, physical keyboard input, and optional host audio.

use std::io::Write;

use super::*;
use crate::cartridge::window::{Session, Update};

/// Host-only playback policy. The CLI validates integer speeds from 1 through 16.
pub struct Playback {
    pub audio: bool,
    pub speed: u32,
}

impl Playback {
    fn audio_enabled(&self) -> bool {
        self.audio && self.speed == 1
    }

    fn frame_period(&self) -> Duration {
        Duration::from_secs_f64(
            f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ) / f64::from(self.speed),
        )
    }
}

fn next_frame_start(previous: Instant, now: Instant, period: Duration, audio: bool) -> Instant {
    let target = previous + period;
    if audio && now.saturating_duration_since(target) <= period {
        target
    } else {
        now
    }
}

pub fn run(
    bytes: Vec<u8>,
    frame_limit: Option<u64>,
    hardware: gba_core::cartridge::CartridgeHardware,
    save_device: gba_core::cartridge::SaveDevice,
    playback: Playback,
    save_file: Option<&mut crate::cartridge::save_file::SaveFile>,
    writer: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let audio_enabled = playback.audio_enabled();
    let mut session = Session::new(bytes)?;
    session.set_cartridge_hardware(hardware);
    session.memory_mut().set_save_device(save_device);
    if let Some(save) = save_file.as_ref() {
        save.initialize(session.memory_mut())?;
    }
    let mut rtc_clock = if hardware == gba_core::cartridge::CartridgeHardware::Rtc {
        Some(crate::cartridge::rtc_clock::RtcHostClock::new(
            session.memory_mut(),
        )?)
    } else {
        None
    };
    writeln!(
        writer,
        "ROM window: Arrows=D-pad; Z/X=A/B; Q/W=L/R; Enter=Start; Backspace=Select; Escape exits."
    )?;
    writeln!(
        writer,
        "Input releases and audio mutes on focus loss. Save files require --save-file and clean exit. Startup frames are not suppressed."
    )?;
    writeln!(
        writer,
        "Playback speed: {}x target; actual speed depends on host performance.",
        playback.speed
    )?;
    if let Some(limit) = frame_limit {
        writeln!(
            writer,
            "Window frame limit: {limit}; STOP or early closure is an error."
        )?;
    }
    writer.flush()?;
    let mut audio = None;
    let mut samples = [gba_core::audio::StereoLevel::default(); 1024];
    let result = (|| -> Result<&str, Box<dyn Error>> {
        let title = format!("GBA Rust | ROM | {}x target | Esc: exit", playback.speed);
        let mut window = create_window(&title)?;
        window.set_target_fps(0);
        if audio_enabled {
            let output = crate::audio::AudioOutput::new()?;
            writeln!(writer, "Audio enabled: {}", output.description())?;
            audio = Some(output);
            session.memory_mut().set_audio_capture(true);
        } else if playback.speed > 1 {
            writeln!(
                writer,
                "Audio muted for accelerated playback (--audio ignored above 1x)."
            )?;
        } else {
            writeln!(writer, "Audio muted; use --audio for macOS output.")?;
        }
        writer.flush()?;
        let mut was_audio_active = false;
        let period = playback.frame_period();
        // STOP has no emulation work to accelerate. Keep host input polling at normal speed.
        let stopped_period = Playback {
            audio: false,
            speed: 1,
        }
        .frame_period();
        let mut frame_started = Instant::now();
        let mut was_stopped = false;
        // Show a black buffer and pump initial events before boot starts.
        window.update_with_buffer(session.frame().pixels(), WIDTH, HEIGHT)?;
        while window.is_open() && !window.is_key_down(Key::Escape) {
            if let Some(clock) = rtc_clock.as_mut() {
                clock.sync(session.memory_mut())?;
            }
            let reported_active = window.is_active();
            let focused = is_focused(reported_active);
            let buttons = read_buttons(reported_active, |key| window.is_key_down(key));
            let update = session.update(buttons)?;
            let stopped = update == Update::Stopped;
            if let Some(output) = audio.as_ref() {
                let active = focused && !stopped;
                if !active && was_audio_active {
                    output.clear()?;
                }
                was_audio_active = active;
                loop {
                    let count = session.memory_mut().drain_audio_samples(&mut samples);
                    output.submit(if active { &samples[..count] } else { &[] })?;
                    if count < samples.len() {
                        break;
                    }
                }
                if session.memory_mut().audio_dropped_samples() != 0 {
                    return Err(
                        io::Error::other("core audio capture overflow; reduce host load").into(),
                    );
                }
            }
            if stopped != was_stopped {
                window.set_title(if stopped {
                    "GBA Rust | ROM STOP: waiting for enabled keypad input | Esc: exit"
                } else {
                    &title
                });
                was_stopped = stopped;
            }
            match update {
                Update::Running => {
                    // Always service native events between bounded CPU slices.
                    window.update();
                }
                Update::Frame => {
                    window.update_with_buffer(session.frame().pixels(), WIDTH, HEIGHT)?;
                    if frame_limit.is_some_and(|limit| session.frames() >= limit) {
                        return Ok("window frame limit reached");
                    }
                    thread::sleep(period.saturating_sub(frame_started.elapsed()));
                    // With audio, retain an absolute frame deadline so sleep overshoot
                    // does not steadily starve the output device. Catch up at most one
                    // frame; a slow host never accumulates unbounded work or skips emulation.
                    frame_started =
                        next_frame_start(frame_started, Instant::now(), period, audio_enabled);
                }
                Update::Stopped => {
                    if frame_limit.is_some() {
                        return Err(io::Error::other("ROM entered STOP before the window frame limit; use --window without --frames to wait for input").into());
                    }
                    // Retain the last image. Host polling and sleep do not advance GBA clocks.
                    window.update();
                    thread::sleep(stopped_period);
                    frame_started = Instant::now();
                }
            }
        }
        if frame_limit.is_some() {
            return Err(io::Error::other("ROM window closed before the frame limit").into());
        }
        Ok("ROM window closed")
    })();
    session.report(
        writer,
        result.as_ref().copied().unwrap_or("ROM window diagnostic"),
    )?;
    if let Some(output) = audio.as_ref() {
        output.report(writer)?;
    }
    result?;
    writer.flush()?;
    if let Some(save) = save_file {
        crate::cartridge::persist_save(save, session.memory_mut(), writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod pacing_tests {
    use super::*;
    #[test]
    fn speed_scales_only_host_period_and_disables_audio_above_one() {
        let normal = Playback {
            audio: true,
            speed: 1,
        };
        assert!(normal.audio_enabled());
        assert!(!Playback {
            audio: false,
            speed: 1
        }
        .audio_enabled());
        for speed in 1..=16 {
            for audio in [false, true] {
                let playback = Playback { audio, speed };
                assert_eq!(playback.audio_enabled(), audio && speed == 1);
                let total = playback.frame_period().as_secs_f64() * f64::from(speed);
                assert!((total - normal.frame_period().as_secs_f64()).abs() < 0.00000002);
                assert!(!playback.frame_period().is_zero());
            }
        }
    }

    #[test]
    fn audio_deadlines_absorb_sleep_overshoot_but_bound_catch_up() {
        let start = Instant::now();
        let period = Duration::from_millis(16);
        assert_eq!(
            next_frame_start(start, start + Duration::from_millis(17), period, true),
            start + period
        );
        let late = start + Duration::from_millis(50);
        assert_eq!(next_frame_start(start, late, period, true), late);
        let now = start + Duration::from_millis(17);
        assert_eq!(next_frame_start(start, now, period, false), now);
    }
}
