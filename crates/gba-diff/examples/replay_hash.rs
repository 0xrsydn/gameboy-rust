//! Per-frame state hashes for a scripted game replay.
//!
//! Development-only tooling. It boots a ROM, replays a fixed input script, and hashes
//! the machine after every frame: the full save state, CPU registers, and cycle count,
//! plus, with `--capture`, the rendered frame and captured audio. `--record` writes the
//! hashes; `--check` compares a later build against them and names the first frame
//! that differs. A refactor or optimization that claims to keep behavior must keep
//! every hash. `--write-save` and `--load-save` carry the save memory from one replay
//! into the next, so an in-game save and a later CONTINUE can be checked as well.
//! ROMs, saves, and recorded hashes stay under the ignored `roms/` directory; input
//! scripts are plain button timings and live in `tools/replays/`. `--bench` skips the
//! hashes, still presents each frame and drains audio as the window does with `--capture`,
//! and reports the emulation speed of the replay.
//!
//! `--record --states DIR --every K` also writes the save state at the start of every K-th
//! frame to `DIR/FRAME.state`. `--start FRAME --states DIR` resumes from one of them, so a
//! long replay can be checked as parallel segments: each segment's last hash covers the
//! save state the next segment starts from, so together they check every frame.
//! `--png FRAME:PATH` (repeatable, with `--capture`) writes that frame as a PNG image.

use std::{env, fs, path::PathBuf, process::ExitCode, time::Instant};

#[path = "common/png.rs"]
mod png;

use gba_core::{
    bios,
    cartridge::{CartridgeHardware, RtcDateTime, SaveDevice},
    input::{Button, Buttons},
    machine::Machine,
    video::{Framebuffer, HEIGHT, WIDTH},
};

/// Machine steps allowed between VBlanks before a frame is reported as stuck.
const FRAME_STEP_LIMIT: usize = 2_000_000;

const USAGE: &str = "usage: replay_hash --rom ROM --frames N [--save-type sram|flash64|flash128|eeprom512|eeprom8k] [--rtc SECONDS_SINCE_2000] [--inputs FILE] [--capture] [--idle-skip] [--load-save FILE] [--write-save FILE] [--record FILE | --check FILE | --bench] [--states DIR [--every K | --start FRAME]] [--png FRAME:PATH]...";

struct Options {
    rom: PathBuf,
    frames: u64,
    save: SaveDevice,
    rtc: Option<u64>,
    inputs: Option<PathBuf>,
    capture: bool,
    idle_skip: bool,
    load_save: Option<PathBuf>,
    write_save: Option<PathBuf>,
    record: Option<PathBuf>,
    check: Option<PathBuf>,
    bench: bool,
    states: Option<PathBuf>,
    every: Option<u64>,
    start: u64,
    png: Vec<(u64, PathBuf)>,
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
        load_save: None,
        write_save: None,
        record: None,
        check: None,
        bench: false,
        states: None,
        every: None,
        start: 0,
        png: Vec::new(),
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
            "--load-save" => options.load_save = Some(value()?.into()),
            "--write-save" => options.write_save = Some(value()?.into()),
            "--record" => options.record = Some(value()?.into()),
            "--check" => options.check = Some(value()?.into()),
            "--bench" => options.bench = true,
            "--states" => options.states = Some(value()?.into()),
            "--every" => options.every = Some(number(value()?)?),
            "--start" => options.start = number(value()?)?,
            "--png" => {
                let text = value()?;
                let (frame, path) = text
                    .split_once(':')
                    .ok_or(format!("--png expects FRAME:PATH, got {text}"))?;
                options.png.push((number(frame.to_owned())?, path.into()));
            }
            _ => return Err(format!("unknown option: {arg}\n{USAGE}")),
        }
    }
    if options.rom.as_os_str().is_empty() || options.frames == 0 {
        return Err(USAGE.to_owned());
    }
    if [
        options.record.is_some(),
        options.check.is_some(),
        options.bench,
    ]
    .iter()
    .filter(|&&set| set)
    .count()
        > 1
    {
        return Err("use one of --record, --check, and --bench".to_owned());
    }
    if !options.png.is_empty() && (!options.capture || options.bench) {
        return Err("--png needs --capture and rejects --bench".to_owned());
    }
    if let Some((frame, _)) = options
        .png
        .iter()
        .find(|(frame, _)| *frame < options.start || *frame >= options.frames)
    {
        return Err(format!(
            "--png frame {frame} is outside the replayed frames"
        ));
    }
    if options.every.is_some() && (options.record.is_none() || options.states.is_none()) {
        return Err("--every needs --record and --states".to_owned());
    }
    if options.every == Some(0) {
        return Err("--every needs a positive frame count".to_owned());
    }
    if options.start != 0 {
        if options.states.is_none() || options.record.is_some() {
            return Err("--start needs --states and --check or --bench".to_owned());
        }
        if options.load_save.is_some() || options.write_save.is_some() {
            return Err(
                "--start resumes a save state; it rejects --load-save and --write-save".to_owned(),
            );
        }
        if options.start >= options.frames {
            return Err("--start must be below --frames".to_owned());
        }
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

/// FNV-1a, 64-bit: fixed across toolchains, unlike the standard library's hasher. The
/// save-state checksum is the same function.
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

fn frame_hash(
    machine: &mut Machine,
    state: &[u8],
    capture: Option<&mut Framebuffer>,
) -> Result<u64, String> {
    let mut hash = Fnv::new();
    // A save state ends with the FNV-1a checksum of everything before it, so its last
    // eight bytes stand for the whole state without hashing it a second time.
    let checksum = state
        .len()
        .checked_sub(8)
        .ok_or("save state shorter than its checksum")?;
    hash.bytes(&state[checksum..]);
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

/// Where `--states` keeps the save state for the start of `frame`.
fn state_path(options: &Options, frame: u64) -> PathBuf {
    options
        .states
        .as_ref()
        .expect("--states")
        .join(format!("{frame}.state"))
}

/// The buttons held during each frame.
fn held_buttons(inputs: &[(u64, Button, u64)], frames: u64) -> Vec<Buttons> {
    let mut held = vec![Buttons::default(); frames as usize];
    for &(start, button, hold) in inputs {
        let end = start.saturating_add(hold).min(frames);
        for buttons in held.iter_mut().take(end as usize).skip(start as usize) {
            *buttons = buttons.with(button, true);
        }
    }
    held
}

/// Run the replay without hashing and report its speed. With `capture`, present every
/// frame and drain audio as the window does.
fn bench(
    machine: &mut Machine,
    held: &[Buttons],
    capture: bool,
    frame: &mut Framebuffer,
) -> Result<bool, String> {
    let started = Instant::now();
    let mut samples = [Default::default(); 4096];
    for (index, &buttons) in held.iter().enumerate() {
        machine.memory_mut().set_buttons(buttons);
        machine
            .run_until_vblank(FRAME_STEP_LIMIT)
            .map_err(|error| {
                format!(
                    "frame {index}: {error:?} at PC={:#010x}",
                    machine.cpu().pc()
                )
            })?;
        if capture {
            machine
                .memory()
                .present_frame(frame)
                .map_err(|error| format!("frame capture: {error:?}"))?;
            while machine.memory_mut().drain_audio_samples(&mut samples) != 0 {}
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let fps = held.len() as f64 / seconds;
    println!(
        "{} frames in {seconds:.2} s: {fps:.1} fps ({:.2}x real time)",
        held.len(),
        fps / 59.7275
    );
    Ok(true)
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
    if let Some(path) = &options.load_save {
        let image = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        machine
            .memory_mut()
            .load_save_image(&image)
            .map_err(|error| format!("{}: {error:?}", path.display()))?;
    }
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
    if options.start != 0 {
        let path = state_path(&options, options.start);
        let state = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        machine
            .load_state(&state)
            .map_err(|error| format!("{}: {error:?}", path.display()))?;
    }
    machine.set_idle_skip(options.idle_skip);
    let mut frame = Framebuffer::default();
    let held = held_buttons(&inputs, options.frames);
    let start = options.start as usize;

    if options.bench {
        return bench(&mut machine, &held[start..], options.capture, &mut frame);
    }
    if let (Some(dir), Some(_)) = (&options.states, options.every) {
        fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }

    let mut lines = Vec::with_capacity(options.frames as usize);
    let mut total = Fnv::new();
    for index in options.start..options.frames {
        machine.memory_mut().set_buttons(held[index as usize]);
        machine
            .run_until_vblank(FRAME_STEP_LIMIT)
            .map_err(|error| {
                format!(
                    "frame {index}: {error:?} at PC={:#010x}",
                    machine.cpu().pc()
                )
            })?;
        let state = machine
            .save_state()
            .map_err(|error| format!("save state: {error:?}"))?;
        let next = index + 1;
        if options.every.is_some_and(|every| next % every == 0) && next < options.frames {
            let path = state_path(&options, next);
            fs::write(&path, &state).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        let hash = frame_hash(&mut machine, &state, options.capture.then_some(&mut frame))?;
        for (_, path) in options.png.iter().filter(|(frame, _)| *frame == index) {
            let image = png::encode(WIDTH, HEIGHT, frame.pixels());
            fs::write(path, image).map_err(|error| format!("{}: {error}", path.display()))?;
        }
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
    if let Some(path) = &options.write_save {
        let memory = machine.memory();
        if memory.save_write_pending() {
            return Err("a save write is still in progress at the last frame".to_owned());
        }
        let image = memory
            .save_image()
            .ok_or("--write-save needs --save-type")?;
        fs::write(path, image).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if let Some(path) = &options.record {
        fs::write(path, lines.join("\n") + "\n")
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let frames = options.frames - options.start;
    println!("{frames} frames, combined hash {:016x}", total.0);
    if expected.is_some() {
        match options.start {
            0 => println!("all {frames} frames match"),
            start => println!(
                "all {frames} frames match (frames {start}-{})",
                options.frames - 1
            ),
        }
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
