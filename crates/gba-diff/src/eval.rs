//! `--eval`: score frames the way the GBA Eval benchmark lines them up. This emulator runs
//! through `gba_emu::Session`, the code its WebAssembly build exports; Mesen runs in
//! mesen-ref's eval mode, as the benchmark's reference configures it. Both burn the boot
//! frames, then take the replay's keys before each frame. Only the picture is compared,
//! after 5-bit quantization, as the benchmark's metric does; audio is not compared.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use gba_emu::{Session, BOOT_FRAMES};

use crate::reference::Reference;

/// Key changes from a GBA Eval replay: `<frame> <keys_hex>` lines, `#` comments.
pub fn parse_replay(text: &str) -> Result<Vec<(u64, u16)>, String> {
    let mut events = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(frame), Some(keys)) = (parts.next(), parts.next()) else {
            return Err(format!("replay line without frame and keys: {line}"));
        };
        let frame = frame
            .parse()
            .map_err(|_| format!("replay frame: {frame}"))?;
        let keys = u16::from_str_radix(keys.trim_start_matches("0x"), 16)
            .map_err(|_| format!("replay keys: {keys}"))?;
        events.push((frame, keys));
    }
    events.sort_by_key(|&(frame, _)| frame);
    Ok(events)
}

/// Keys held during `frame`: the latest event at or before it.
pub fn keys_at(events: &[(u64, u16)], frame: u64) -> u16 {
    events
        .iter()
        .take_while(|&&(start, _)| start <= frame)
        .last()
        .map_or(0, |&(_, keys)| keys)
}

/// 0xAABBGGRR to RGB555.
fn rgb555(pixel: u32) -> u16 {
    let red = (pixel & 0xff) >> 3;
    let green = (pixel >> 8 & 0xff) >> 3;
    let blue = (pixel >> 16 & 0xff) >> 3;
    (red | green << 5 | blue << 10) as u16
}

pub struct Summary {
    pub frames: u64,
    pub identical: u64,
    /// Differing pixels summed over all frames.
    pub differing_pixels: u64,
    pub last_frame_pixels: usize,
    pub first: Vec<(u64, usize)>,
    pub error: Option<String>,
}

pub fn run(
    reference: &mut Reference,
    rom_path: &Path,
    bios_path: &Path,
    frames: u64,
    replay: &[(u64, u16)],
    out: &mut impl Write,
) -> Result<Summary, String> {
    let report = |error: io::Error| error.to_string();
    let rom = fs::read(rom_path).map_err(|error| format!("{}: {error}", rom_path.display()))?;
    let bios = fs::read(bios_path).map_err(|error| format!("{}: {error}", bios_path.display()))?;
    let mut session = Session::new(bios);
    session.load(rom)?;
    reference.load_eval(rom_path, bios_path).map_err(report)?;
    for _ in 0..BOOT_FRAMES {
        session.run_frame();
        session.drain_audio();
        reference.run_frame().map_err(report)?;
    }
    let mut summary = Summary {
        frames,
        identical: 0,
        differing_pixels: 0,
        last_frame_pixels: 0,
        first: Vec::new(),
        error: None,
    };
    for frame in 0..frames {
        let keys = keys_at(replay, frame);
        session.set_keys(keys);
        reference.set_keys(keys).map_err(report)?;
        session.run_frame();
        session.drain_audio();
        reference.run_frame().map_err(report)?;
        let theirs = reference.frame().map_err(report)?;
        let differing = session
            .pixels()
            .iter()
            .zip(&theirs)
            .filter(|&(&ours, &theirs)| rgb555(ours) != theirs)
            .count();
        if differing == 0 {
            summary.identical += 1;
        } else if summary.first.len() < 8 {
            summary.first.push((frame, differing));
        }
        summary.differing_pixels += differing as u64;
        summary.last_frame_pixels = differing;
        if summary.error.is_none() {
            if let Some(error) = session.error() {
                writeln!(out, "frame {frame}: this emulator stopped: {error}").map_err(report)?;
                summary.error = Some(error.to_owned());
            }
        }
    }
    let pixels = (frames.max(1) * 240 * 160) as f64;
    writeln!(
        out,
        "eval: {} frames, {} identical ({:.1}%), {:.3}% of pixels differ; last frame {} pixels; first differing (frame, pixels): {:?}",
        summary.frames,
        summary.identical,
        100.0 * summary.identical as f64 / frames.max(1) as f64,
        100.0 * summary.differing_pixels as f64 / pixels,
        summary.last_frame_pixels,
        summary.first
    )
    .map_err(report)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_parse_like_the_benchmark_and_hold_keys_until_the_next_event() {
        let events = parse_replay(
            "# Format: <frame> <keys_hex>\n0 0000\n79 0001 # A\n\n89 0000\n528 0050\n",
        )
        .unwrap();
        assert_eq!(events, vec![(0, 0), (79, 1), (89, 0), (528, 0x50)]);
        assert_eq!(keys_at(&events, 78), 0);
        assert_eq!(keys_at(&events, 79), 1);
        assert_eq!(keys_at(&events, 88), 1);
        assert_eq!(keys_at(&events, 600), 0x50);
        assert_eq!(keys_at(&[], 5), 0);
        assert!(parse_replay("5").is_err());
        assert!(parse_replay("x 1").is_err());
    }

    #[test]
    fn pixels_compare_in_rgb555() {
        assert_eq!(rgb555(0xff00_00f8), 0x001f);
        assert_eq!(rgb555(0xff00_ff00), 0x03e0);
        assert_eq!(rgb555(0xffff_0000), 0x7c00);
        assert_eq!(rgb555(0xff07_0707), 0); // The low three bits of each channel are ignored.
    }
}
