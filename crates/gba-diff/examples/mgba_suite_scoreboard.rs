//! Headless scoreboard for the pinned mGBA test suite ROM.
//!
//! Development-only tooling. It boots the suite ROM, runs each of the 14 sub-suites
//! from a fresh boot, and prints the pass/result counters that the suite keeps in
//! memory. It does not decide pass or fail: the counters are the suite's own report.
//! A counter that stops changing is a settled result; a timeout is never a pass.
//!
//! The suite ROM is not distributed with this repository. Build it as
//! `docs/public-mgba-suite.md` describes; the layout constants below belong to that
//! pinned revision and its published SHA-256.

use std::{env, fs, path::PathBuf, process::ExitCode, time::Instant};

use gba_core::{
    bios,
    cartridge::SaveDevice,
    input::{Button, Buttons},
    machine::Machine,
    video::Framebuffer,
};

/// `struct TestSuite` pointer table in the pinned ROM revision.
const SUITES_TABLE: u32 = 0x0803_d868;
/// Offsets inside `struct TestSuite`: name, run, list, show, nTests, passes, total.
const NAME_OFFSET: u32 = 0;
const TEST_COUNT_OFFSET: u32 = 16;
const PASSES_OFFSET: u32 = 20;
const TOTAL_RESULTS_OFFSET: u32 = 24;
const SRAM_BASE: u32 = 0x0e00_0000;
const SRAM_SIZE: usize = 0x8000;
/// The suite keeps its counters in IWRAM.
const IWRAM: std::ops::Range<u32> = 0x0300_0000..0x0300_8000;
const ROM_START: u32 = 0x0800_0000;
/// Machine steps allowed between VBlanks before a frame is reported as stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;
/// Frames to let the firmware and menu settle before the first key press.
const BOOT_FRAMES: usize = 60;
/// Frames to hold a button and to release it between taps.
const KEY_TAP_FRAMES: usize = 2;
/// Suites in the pinned revision; the pointer table has this many entries.
const SUITE_COUNT: usize = 14;
/// Frames to let a visual test's menu or picture settle before the next step.
const VIDEO_SETTLE_FRAMES: usize = 20;

const USAGE: &str =
    "usage: mgba-suite-scoreboard --rom PATH [--settle FRAMES] [--frame-cap FRAMES] [--log PATH]";

struct Options {
    rom: PathBuf,
    settle: u64,
    frame_cap: u64,
    log: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut rom = None;
    let mut settle = 240;
    let mut frame_cap = 18_000;
    let mut log = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut number = |max: u64| -> Result<u64, String> {
            args.next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| (1..=max).contains(value))
                .ok_or_else(|| USAGE.to_owned())
        };
        match arg.as_str() {
            "--rom" => rom = args.next().map(PathBuf::from),
            "--settle" => settle = number(100_000)?,
            "--frame-cap" => frame_cap = number(10_000_000)?,
            "--log" => log = args.next().map(PathBuf::from),
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(Options {
        rom: rom.ok_or_else(|| USAGE.to_owned())?,
        settle,
        frame_cap,
        log,
    })
}

struct Suite {
    name: String,
    passes: u32,
    total_results: u32,
    /// False for the visual-only suite, whose counters are not live IWRAM variables.
    counted: bool,
    /// Entries in the sub-suite's own test list.
    tests: u32,
}

/// Read a little-endian word from the loaded ROM image at `address`.
fn rom_u32(rom: &[u8], address: u32) -> Result<u32, String> {
    let offset = address.wrapping_sub(ROM_START) as usize;
    let bytes = rom
        .get(offset..offset + 4)
        .ok_or_else(|| format!("ROM is too small for address {address:#010x}"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a NUL-terminated ASCII string from the loaded ROM image.
fn rom_string(rom: &[u8], address: u32) -> Result<String, String> {
    let offset = address.wrapping_sub(ROM_START) as usize;
    let mut bytes = Vec::new();
    for index in 0..96 {
        let byte = *rom
            .get(offset + index)
            .ok_or_else(|| format!("unterminated string at {address:#010x}"))?;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_suites(rom: &[u8]) -> Result<Vec<Suite>, String> {
    let mut suites = Vec::with_capacity(SUITE_COUNT);
    for index in 0..SUITE_COUNT {
        let entry = rom_u32(rom, SUITES_TABLE + index as u32 * 4)?;
        let name = rom_string(rom, rom_u32(rom, entry + NAME_OFFSET)?)?;
        let passes = rom_u32(rom, entry + PASSES_OFFSET)?;
        let total_results = rom_u32(rom, entry + TOTAL_RESULTS_OFFSET)?;
        let counted = IWRAM.contains(&passes) && IWRAM.contains(&total_results);
        suites.push(Suite {
            name,
            passes,
            total_results,
            counted,
            tests: rom_u32(rom, entry + TEST_COUNT_OFFSET)?,
        });
    }
    Ok(suites)
}

fn run_frame(machine: &mut Machine) -> Result<(), String> {
    let previous = machine.memory().display_position().vblanks;
    for _ in 0..FRAME_STEP_LIMIT {
        machine.step().map_err(|error| format!("{error:?}"))?;
        if machine.memory().display_position().vblanks != previous {
            return Ok(());
        }
    }
    Err(format!("no VBlank within {FRAME_STEP_LIMIT} machine steps"))
}

fn tap(machine: &mut Machine, button: Button) -> Result<(), String> {
    machine
        .memory_mut()
        .set_buttons(Buttons::default().with(button, true));
    for _ in 0..KEY_TAP_FRAMES {
        run_frame(machine)?;
    }
    machine.memory_mut().set_buttons(Buttons::default());
    for _ in 0..KEY_TAP_FRAMES {
        run_frame(machine)?;
    }
    Ok(())
}

fn read_counter(machine: &Machine, address: u32) -> Result<u32, String> {
    machine
        .memory()
        .read32(address)
        .map_err(|error| error.to_string())
}

/// Failure lines the suite writes to SRAM. The same buffer also holds informational
/// lines such as a test name before it runs, so keep only the lines that report a mismatch.
fn sram_failures(machine: &Machine) -> Result<Vec<String>, String> {
    let mut text = Vec::with_capacity(SRAM_SIZE);
    for offset in 0..SRAM_SIZE as u32 {
        let byte = machine
            .memory()
            .read8(SRAM_BASE + offset)
            .map_err(|error| error.to_string())?;
        if byte == 0 {
            break;
        }
        text.push(byte);
    }
    let text = String::from_utf8_lossy(&text);
    // The suite logs a "<sub-suite> test: <name>" header before each test's failures.
    // Carry the name onto every failure line; a bare line names only the configuration.
    let mut test = "";
    let mut failures = Vec::new();
    for line in text.lines().skip(2).map(str::trim_end) {
        if line.contains(": FAIL") {
            failures.push(if test.is_empty() {
                line.to_owned()
            } else {
                format!("{test} | {line}")
            });
        } else if let Some((_, name)) = line.split_once(" test: ") {
            test = name;
        }
    }
    Ok(failures)
}

struct Score {
    passes: u32,
    total_results: u32,
    frames: u64,
    settled: bool,
}

/// Boot the ROM, select one sub-suite, and run until its counters stop changing.
fn score_suite(
    rom: &[u8],
    suite: &Suite,
    index: usize,
    options: &Options,
) -> Result<(Score, Vec<String>), String> {
    let mut machine = bios::boot(rom.to_vec()).map_err(|error| error.to_string())?;
    machine.memory_mut().set_save_device(SaveDevice::Sram);
    for _ in 0..BOOT_FRAMES {
        run_frame(&mut machine)?;
    }
    for _ in 0..index {
        tap(&mut machine, Button::Down)?;
    }
    tap(&mut machine, Button::A)?;
    let mut frames = 0u64;
    let mut previous = 0u32;
    let mut stable = 0u64;
    loop {
        let total = read_counter(&machine, suite.total_results)?;
        if total == previous {
            stable += 1;
        } else {
            stable = 0;
            previous = total;
        }
        if total > 0 && stable >= options.settle {
            break;
        }
        if frames >= options.frame_cap {
            break;
        }
        run_frame(&mut machine)?;
        frames += 1;
    }
    let score = Score {
        passes: read_counter(&machine, suite.passes)?,
        total_results: read_counter(&machine, suite.total_results)?,
        frames,
        settled: stable >= options.settle,
    };
    Ok((score, sram_failures(&machine)?))
}

fn frames(machine: &mut Machine, count: usize) -> Result<(), String> {
    for _ in 0..count {
        run_frame(machine)?;
    }
    Ok(())
}

fn capture(machine: &Machine) -> Result<Framebuffer, String> {
    let mut frame = Framebuffer::default();
    match machine.memory().present_frame(&mut frame) {
        Ok(true) => Ok(frame),
        Ok(false) => Err("no captured frame".to_owned()),
        Err(error) => Err(format!("{error:?}")),
    }
}

/// Open one screen of the visual sub-suite and count the pixels in which its
/// "actual" rendering differs from the "expected" picture the ROM draws for it.
/// Both come from this emulator: the expected picture uses only static tiles,
/// so a count of zero means the tested effect was rendered as on hardware.
fn score_video(rom: &[u8], index: usize, test: u32) -> Result<usize, String> {
    let mut machine = bios::boot(rom.to_vec()).map_err(|error| error.to_string())?;
    machine.memory_mut().set_save_device(SaveDevice::Sram);
    machine.memory_mut().set_scanline_rendering(true);
    frames(&mut machine, BOOT_FRAMES)?;
    for _ in 0..index {
        tap(&mut machine, Button::Down)?;
    }
    tap(&mut machine, Button::A)?;
    frames(&mut machine, VIDEO_SETTLE_FRAMES)?;
    for _ in 0..test {
        tap(&mut machine, Button::Down)?;
    }
    tap(&mut machine, Button::A)?;
    frames(&mut machine, VIDEO_SETTLE_FRAMES)?;
    tap(&mut machine, Button::Start)?; // Hide the "actual"/"expected" label sprite.
    frames(&mut machine, VIDEO_SETTLE_FRAMES)?;
    let actual = capture(&machine)?;
    tap(&mut machine, Button::Right)?;
    frames(&mut machine, VIDEO_SETTLE_FRAMES)?;
    let expected = capture(&machine)?;
    Ok(actual
        .pixels()
        .iter()
        .zip(expected.pixels())
        .filter(|(actual, expected)| actual != expected)
        .count())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let rom = match fs::read(&options.rom) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("cannot read ROM {:?}: {error}", options.rom);
            return ExitCode::from(2);
        }
    };
    let suites = match read_suites(&rom) {
        Ok(suites) => suites,
        Err(message) => {
            eprintln!("cannot read the suite table: {message}");
            return ExitCode::from(2);
        }
    };
    println!("ROM: {:?} ({} bytes)", options.rom, rom.len());
    println!(
        "Settle window: {} frames; frame cap: {} frames",
        options.settle, options.frame_cap
    );
    println!();
    println!(
        "{:<3} {:<20} {:>10} {:>10} {:>8}  Status",
        "#", "Sub-suite", "Passed", "Results", "Frames"
    );
    let mut failures = Vec::new();
    let mut failed = false;
    for (index, suite) in suites.iter().enumerate() {
        if !suite.counted {
            let mut differing = Vec::new();
            for test in 0..suite.tests {
                match score_video(&rom, index, test) {
                    Ok(0) => {}
                    Ok(pixels) => differing.push(format!("screen {test}: {pixels} pixels differ")),
                    Err(error) => {
                        differing.push(format!("screen {test}: {error}"));
                        failed = true;
                    }
                }
            }
            println!(
                "{index:<3} {:<20} {:>10} {:>10} {:>8}  actual against expected picture",
                suite.name,
                suite.tests as usize - differing.len(),
                suite.tests,
                "-"
            );
            if !differing.is_empty() {
                failures.push((suite.name.clone(), differing));
            }
            continue;
        }
        let started = Instant::now();
        let (score, suite_failures) = match score_suite(&rom, suite, index, &options) {
            Ok(result) => result,
            Err(error) => {
                println!(
                    "{index:<3} {:<20} {:>10} {:>10} {:>8}  {error}",
                    suite.name, "-", "-", "-"
                );
                failed = true;
                continue;
            }
        };
        let status = if score.settled {
            format!("settled in {:.1} s", started.elapsed().as_secs_f64())
        } else {
            failed = true;
            "did not settle (cap)".to_owned()
        };
        println!(
            "{index:<3} {:<20} {:>10} {:>10} {:>8}  {status}",
            suite.name, score.passes, score.total_results, score.frames
        );
        if !suite_failures.is_empty() {
            failures.push((suite.name.clone(), suite_failures));
        }
    }
    if !failures.is_empty() {
        println!();
        println!("Recorded failure lines (from SRAM, as the suite logged them):");
        for (name, lines) in &failures {
            println!("[{name}] {} line(s)", lines.len());
            for line in lines.iter().take(12) {
                println!("  {line}");
            }
            if lines.len() > 12 {
                println!("  ... {} more", lines.len() - 12);
            }
        }
    }
    if let Some(path) = &options.log {
        let text = failures
            .iter()
            .map(|(name, lines)| format!("[{name}]\n{}\n", lines.join("\n")))
            .collect::<String>();
        if let Err(error) = fs::write(path, text) {
            eprintln!("cannot write log {path:?}: {error}");
            failed = true;
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
