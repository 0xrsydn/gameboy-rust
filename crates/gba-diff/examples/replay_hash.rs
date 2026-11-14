//! Per-frame state hashes for a scripted game replay.
//!
//! Development-only tooling. It boots a ROM, replays a fixed input script, and hashes
//! the machine after every frame: the full save state, CPU registers, and cycle count,
//! plus, with `--capture`, the rendered frame and captured audio. `--record` writes the
//! hashes; `--check` compares a later build against them and names the first frame
//! that differs. A refactor or optimization that claims to keep behavior must keep
//! every hash. ROMs and recorded hashes stay under the ignored `roms/` directory;
//! input scripts are plain button timings and live in `tools/replays/`.

use std::{env, fs, path::PathBuf, process::ExitCode};

use gba_core::{
    bios,
    cartridge::{CartridgeHardware, RtcDateTime, SaveDevice},
    input::{Button, Buttons},
    machine::Machine,
    video::Framebuffer,
};

/// Machine steps allowed between VBlanks before a frame is reported as stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;

const USAGE: &str = "usage: replay_hash --rom ROM --frames N [--save-type sram|flash64|flash128|eeprom512|eeprom8k] [--rtc SECONDS_SINCE_2000] [--inputs FILE] [--capture] [--idle-skip] [--record FILE | --check FILE]";

struct Options {
    rom: PathBuf,
    frames: u64,
    save: SaveDevice,
    rtc: Option<u64>,
    inputs: Option<PathBuf>,
    capture: bool,
    idle_skip: bool,
    record: Option<PathBuf>,
    check: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        rom: PathBuf::new(),
        frames: 0,
        save: SaveDevice::None,
        rtc: None,
        inputs: None,
        capture: false,
        idle_skip: false,
        record: None,
        check: None,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        let number = |text: String| {
            text.parse::<u64>()
                .map_err(|error| format!("{text}: {error}"))
        };
        match arg.as_str() {
            "--rom" => options.rom = value()?.into(),
            "--frames" => options.frames = number(value()?)?,
            "--save-type" => {
                options.save = match value()?.as_str() {
                    "sram" => SaveDevice::Sram,
                    "flash64" => SaveDevice::Flash64,
                    "flash128" => SaveDevice::Flash128,
                    "eeprom512" => SaveDevice::Eeprom512,
                    "eeprom8k" => SaveDevice::Eeprom8k,
                    other => return Err(format!("unknown save type: {other}")),
                }
            }
            "--rtc" => options.rtc = Some(number(value()?)?),
            "--inputs" => options.inputs = Some(value()?.into()),
            "--capture" => options.capture = true,
            "--idle-skip" => options.idle_skip = true,
            "--record" => options.record = Some(value()?.into()),
            "--check" => options.check = Some(value()?.into()),
            _ => return Err(format!("unknown option: {arg}\n{USAGE}")),
        }
    }
    if options.rom.as_os_str().is_empty() || options.frames == 0 {
        return Err(USAGE.to_owned());
    }
    if options.record.is_some() && options.check.is_some() {
        return Err("use --record or --check, not both".to_owned());
    }
    Ok(options)
}

fn button(name: &str) -> Result<Button, String> {
    Ok(match name {
        "a" => Button::A,
        "b" => Button::B,
        "select" => Button::Select,
        "start" => Button::Start,
        "right" => Button::Right,
        "left" => Button::Left,
        "up" => Button::Up,
        "down" => Button::Down,
        "r" => Button::R,
        "l" => Button::L,
        _ => return Err(format!("unknown button: {name}")),
    })
}

/// `FRAME:BUTTON:HOLD` entries separated by commas or newlines; `#` starts a comment.
/// Frames count from the cartridge entry's first VBlank wait, as in `gba-diff`.
fn parse_inputs(text: &str) -> Result<Vec<(u64, Button, u64)>, String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let fields: Vec<&str> = entry.split(':').collect();
            let [frame, name, hold] = fields[..] else {
                return Err(format!("expected FRAME:BUTTON:HOLD, got {entry}"));
            };
            let number = |text: &str| {
                text.parse::<u64>()
                    .map_err(|error| format!("{entry}: {error}"))
            };
            Ok((number(frame)?, button(name)?, number(hold)?))
        })
        .collect()
}

/// FNV-1a, 64-bit: fixed across toolchains, unlike the standard library's hasher.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn frame_hash(machine: &mut Machine, capture: Option<&mut Framebuffer>) -> Result<u64, String> {
    let mut hash = Fnv::new();
    let state = machine
        .save_state()
        .map_err(|error| format!("save state: {error:?}"))?;
    hash.bytes(&state);
    for register in machine.cpu().registers() {
        hash.bytes(&register.to_le_bytes());
    }
    hash.bytes(&machine.cycles().to_le_bytes());
    if let Some(frame) = capture {
        machine
            .memory()
            .present_frame(frame)
            .map_err(|error| format!("frame capture: {error:?}"))?;
        for pixel in frame.pixels() {
            hash.bytes(&pixel.to_le_bytes());
        }
        let mut samples = [Default::default(); 4096];
        loop {
            let count = machine.memory_mut().drain_audio_samples(&mut samples);
            if count == 0 {
                break;
            }
            for sample in &samples[..count] {
                let gba_core::audio::StereoLevel { left, right } = *sample;
                hash.bytes(&left.to_le_bytes());
                hash.bytes(&right.to_le_bytes());
            }
        }
    }
    Ok(hash.0)
}

fn run(options: Options) -> Result<bool, String> {
    let rom =
        fs::read(&options.rom).map_err(|error| format!("{}: {error}", options.rom.display()))?;
    let inputs = match &options.inputs {
        Some(path) => parse_inputs(
            &fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?,
        )?,
        None => Vec::new(),
    };
    let expected = match &options.check {
        Some(path) => {
            Some(fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?)
        }
        None => None,
    };
    let expected: Option<Vec<&str>> = expected.as_deref().map(|text| text.lines().collect());

    let mut machine = bios::boot(rom).map_err(|error| format!("{error:?}"))?;
    machine.memory_mut().set_save_device(options.save);
    if let Some(seconds) = options.rtc {
        machine
            .memory_mut()
            .set_cartridge_hardware(CartridgeHardware::Rtc);
        let date = RtcDateTime::from_seconds_since_2000(seconds)
            .map_err(|error| format!("--rtc: {error:?}"))?;
        machine
            .memory_mut()
            .set_rtc_datetime(date)
            .map_err(|error| format!("--rtc: {error:?}"))?;
    }
    machine.memory_mut().set_scanline_rendering(options.capture);
    machine.memory_mut().set_audio_capture(options.capture);
    machine.set_idle_skip(options.idle_skip);
    let mut frame = Framebuffer::default();

    let mut lines = Vec::with_capacity(options.frames as usize);
    let mut total = Fnv::new();
    for index in 0..options.frames {
        let buttons = inputs
            .iter()
            .filter(|(start, _, hold)| index >= *start && index < start + hold)
            .fold(Buttons::default(), |held, (_, button, _)| {
                held.with(*button, true)
            });
        machine.memory_mut().set_buttons(buttons);
        machine
            .run_until_vblank(FRAME_STEP_LIMIT)
            .map_err(|error| {
                format!(
                    "frame {index}: {error:?} at PC={:#010x}",
                    machine.cpu().pc()
                )
            })?;
        let hash = frame_hash(&mut machine, options.capture.then_some(&mut frame))?;
        total.bytes(&hash.to_le_bytes());
        let line = format!("{index} {hash:016x}");
        if let Some(expected) = &expected {
            match expected.get(index as usize) {
                Some(&wanted) if wanted == line => {}
                Some(&wanted) => {
                    println!(
                        "first difference at frame {index}: expected `{wanted}`, got `{line}`"
                    );
                    return Ok(false);
                }
                None => {
                    println!("recorded hashes end before frame {index}");
                    return Ok(false);
                }
            }
        }
        lines.push(line);
    }
    if let Some(path) = &options.record {
        fs::write(path, lines.join("\n") + "\n")
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    println!("{} frames, combined hash {:016x}", options.frames, total.0);
    if expected.is_some() {
        println!("all {} frames match", options.frames);
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
