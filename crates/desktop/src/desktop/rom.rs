//! Main-thread ROM presentation and physical keyboard input.

use std::io::Write;

use super::*;
use crate::cartridge::window::{Session, Update};

pub fn run(
    bytes: Vec<u8>,
    frame_limit: Option<u64>,
    hardware: gba_core::cartridge::CartridgeHardware,
    writer: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let mut session = Session::new(bytes)?;
    session.set_cartridge_hardware(hardware);
    writeln!(
        writer,
        "ROM window: Arrows=D-pad; Z/X=A/B; Q/W=L/R; Enter=Start; Backspace=Select; Escape exits."
    )?;
    writeln!(
        writer,
        "Input releases on focus loss. No audio or saves. Startup frames are not suppressed."
    )?;
    if let Some(limit) = frame_limit {
        writeln!(
            writer,
            "Window frame limit: {limit}; STOP or early closure is an error."
        )?;
    }
    writer.flush()?;
    let result = (|| -> Result<&str, Box<dyn Error>> {
        let title = "GBA Rust | ROM | Esc: exit | No audio or saves";
        let mut window = create_window(title)?;
        window.set_target_fps(0);
        let period = Duration::from_secs_f64(f64::from(CYCLES_PER_FRAME) / f64::from(CPU_HZ));
        let mut frame_started = Instant::now();
        let mut was_stopped = false;
        // Show a black buffer and pump initial events before boot starts.
        window.update_with_buffer(session.frame().pixels(), WIDTH, HEIGHT)?;
        while window.is_open() && !window.is_key_down(Key::Escape) {
            let buttons = read_buttons(window.is_active(), |key| window.is_key_down(key));
            let update = session.update(buttons)?;
            let stopped = update == Update::Stopped;
            if stopped != was_stopped {
                window.set_title(if stopped {
                    "GBA Rust | ROM STOP: waiting for enabled keypad input | Esc: exit"
                } else {
                    title
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
                    // Slow hosts do not skip emulated frames or accumulate catch-up work.
                    thread::sleep(period.saturating_sub(frame_started.elapsed()));
                    frame_started = Instant::now();
                }
                Update::Stopped => {
                    if frame_limit.is_some() {
                        return Err(io::Error::other("ROM entered STOP before the window frame limit; use --window without --frames to wait for input").into());
                    }
                    // Retain the last image. Host polling and sleep do not advance GBA clocks.
                    window.update();
                    thread::sleep(period);
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
    result.map(|_| ())
}
