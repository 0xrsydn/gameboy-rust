//! Host-only raw ROM loading, execution modes, and diagnostics.

pub(crate) mod rtc_clock;
pub(crate) mod save_file;
pub mod suite;
pub mod window;

use std::{
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use gba_core::{
    bios,
    cartridge::{CartridgeHardware, RtcError, SaveDevice},
    machine::{Machine, MachineError, StepKind},
    memory::ROM_CAPACITY,
};

const MAX_STEPS: u64 = 100_000_000;
const MAX_FRAMES: u64 = 100_000;
const USAGE: &str = "usage: gameboy-rust --rom PATH [--rtc] [--save-type flash64|flash128] [--save-file PATH] (--steps COUNT | --window [--frames COUNT] [--audio] [--speed MULTIPLIER]); speed: integer 1..=16 (default 1, audio muted above 1); steps: 1..=100000000, frames: 1..=100000; do not combine with demo or help options";

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Terminal { steps: u64 },
    Window { frames: Option<u64> },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    path: PathBuf,
    mode: Mode,
    hardware: CartridgeHardware,
    save_device: SaveDevice,
    audio: bool,
    speed: u32,
    save_file: Option<PathBuf>,
}

pub fn parse_args(args: &[OsString]) -> io::Result<Options> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, USAGE);
    let mut path = None;
    let mut steps = None;
    let mut frames = None;
    let mut window = false;
    let mut audio = false;
    let mut speed = None;
    let mut hardware = CartridgeHardware::None;
    let mut save_device = SaveDevice::None;
    let mut save_file = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--rom" && path.is_none() {
            let value = args
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid)?;
            path = Some(PathBuf::from(value));
        } else if arg == "--rtc" && hardware == CartridgeHardware::None {
            hardware = CartridgeHardware::Rtc;
        } else if arg == "--save-type" && save_device == SaveDevice::None {
            save_device = match args.next().and_then(|value| value.to_str()) {
                Some("flash64") => SaveDevice::Flash64,
                Some("flash128") => SaveDevice::Flash128,
                _ => return Err(invalid()),
            };
        } else if arg == "--save-file" && save_file.is_none() {
            save_file = Some(PathBuf::from(
                args.next().filter(|v| !v.is_empty()).ok_or_else(invalid)?,
            ));
        } else if arg == "--speed" && speed.is_none() {
            let value = args
                .next()
                .and_then(|value| value.to_str())
                .ok_or_else(invalid)?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            let multiplier = value.parse::<u32>().map_err(|_| invalid())?;
            if !(1..=16).contains(&multiplier) {
                return Err(invalid());
            }
            speed = Some(multiplier);
        } else if arg == "--audio" && !audio {
            audio = true;
        } else if arg == "--window" && !window {
            window = true;
        } else if (arg == "--steps" && steps.is_none()) || (arg == "--frames" && frames.is_none()) {
            let value = args
                .next()
                .and_then(|value| value.to_str())
                .ok_or_else(invalid)?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            let count = value.parse::<u64>().map_err(|_| invalid())?;
            let max = if arg == "--steps" {
                MAX_STEPS
            } else {
                MAX_FRAMES
            };
            if !(1..=max).contains(&count) {
                return Err(invalid());
            }
            if arg == "--steps" {
                steps = Some(count);
            } else {
                frames = Some(count);
            }
        } else {
            return Err(invalid());
        }
    }
    if save_file.is_some() && save_device == SaveDevice::None {
        return Err(invalid());
    }
    if (audio || speed.is_some()) && !window {
        return Err(invalid());
    }
    let mode = match (window, steps, frames) {
        (false, Some(steps), None) => Mode::Terminal { steps },
        (true, None, frames) => Mode::Window { frames },
        _ => return Err(invalid()),
    };
    Ok(Options {
        path: path.ok_or_else(invalid)?,
        mode,
        hardware,
        save_device,
        save_file,
        audio,
        speed: speed.unwrap_or(1),
    })
}

fn validate_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ROM must be a regular file",
        ));
    }
    if metadata.len() > ROM_CAPACITY as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ROM exceeds the 32 MiB cartridge limit",
        ));
    }
    Ok(())
}

fn read_bounded(reader: impl Read, capacity: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(capacity as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > capacity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ROM exceeds the cartridge byte limit",
        ));
    }
    if bytes.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ROM needs at least 4 bytes for the initial ARM instruction",
        ));
    }
    Ok(bytes)
}

fn load(path: &Path) -> io::Result<Vec<u8>> {
    let result = (|| {
        // Check before opening to reject ordinary directories and named pipes.
        // This is not a guarantee against concurrent filesystem replacement.
        validate_file(&fs::metadata(path)?)?;
        let file = File::open(path)?;
        validate_file(&file.metadata()?)?;
        // Bound the read independently of metadata in case the file grows.
        read_bounded(file, ROM_CAPACITY)
    })();
    result.map_err(|error: io::Error| {
        io::Error::new(error.kind(), format!("cannot load ROM {path:?}: {error}"))
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Stats {
    steps: u64,
    instructions: u64,
    irq_entries: u64,
    dma_units: u64,
    halt_idle: u64,
}

impl Stats {
    fn record(&mut self, kind: StepKind) {
        match kind {
            StepKind::Instruction => self.instructions += 1,
            StepKind::IrqEntry => self.irq_entries += 1,
            StepKind::Dma { .. } => self.dma_units += 1,
            StepKind::HaltIdle => self.halt_idle += 1,
            StepKind::StopIdle => return,
        }
        self.steps += 1;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Reason {
    StepLimit,
    Stopped,
    Diagnostic(MachineError),
}

#[derive(Debug)]
struct Report {
    stats: Stats,
    reason: Reason,
}

#[cfg(test)]
fn run_bounded(machine: &mut Machine, limit: u64) -> Report {
    run_bounded_with_clock(machine, limit, None).unwrap()
}

fn run_bounded_with_clock(
    machine: &mut Machine,
    limit: u64,
    mut clock: Option<&mut rtc_clock::RtcHostClock>,
) -> Result<Report, RtcError> {
    let mut stats = Stats::default();
    let reason = loop {
        if stats.steps % 4096 == 0 {
            if let Some(clock) = clock.as_deref_mut() {
                clock.sync(machine.memory_mut())?;
            }
        }
        // STOP has no host input source here. Do not spin with a frozen clock.
        // Check before the budget so STOP on the last step is reported correctly.
        if machine.stopped() {
            break Reason::Stopped;
        }
        if stats.steps == limit {
            break Reason::StepLimit;
        }
        match machine.step() {
            Ok(StepKind::StopIdle) => break Reason::Stopped,
            Ok(kind) => stats.record(kind),
            Err(error) => break Reason::Diagnostic(error),
        }
    };
    if let Some(clock) = clock {
        clock.sync(machine.memory_mut())?;
    }
    Ok(Report { stats, reason })
}

fn write_report(writer: &mut impl Write, machine: &Machine, report: &Report) -> io::Result<()> {
    let reason = match report.reason {
        Reason::StepLimit => "step limit reached",
        Reason::Stopped => "STOP; input required (no input source in terminal mode)",
        Reason::Diagnostic(_) => "emulation diagnostic",
    };
    writeln!(writer, "Result: {reason}")?;
    write_state(writer, machine, &report.stats)
}

fn write_state(writer: &mut impl Write, machine: &Machine, stats: &Stats) -> io::Result<()> {
    writeln!(
        writer,
        "Steps: {}; instructions: {}; IRQ entries: {}; DMA units: {}; HALT idle: {}",
        stats.steps, stats.instructions, stats.irq_entries, stats.dma_units, stats.halt_idle,
    )?;
    let cpu = machine.cpu();
    writeln!(writer, "Nominal cycles: {}", machine.cycles())?;
    writeln!(
        writer,
        "PC={:#010x} CPSR={:#010x} {:?}/{:?} halted={} stopped={}",
        cpu.pc(),
        cpu.cpsr(),
        cpu.mode(),
        cpu.instruction_set(),
        machine.halted(),
        machine.stopped(),
    )?;
    for (row, registers) in cpu.registers().chunks(4).enumerate() {
        for (column, register) in registers.iter().enumerate() {
            if column != 0 {
                write!(writer, " ")?;
            }
            write!(writer, "r{}={register:#010x}", row * 4 + column)?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

pub fn execute(options: Options, writer: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let bytes = load(&options.path)?;
    writeln!(writer, "ROM: {:?} ({} bytes)", options.path, bytes.len())?;
    writeln!(
        writer,
        "Original BIOS replacement; no commercial-game compatibility claim."
    )?;
    if options.hardware == CartridgeHardware::Rtc {
        writeln!(writer, "RTC selected: UTC at startup, then host elapsed time; no persistence or RTC interrupts.")?;
    }
    if options.save_device != SaveDevice::None {
        writeln!(
            writer,
            "Flash selected: {} KiB Macronix; nominal program/erase timing and DQ7 polling.",
            options.save_device.capacity() / 1024
        )?;
    }
    let mut save_file = options
        .save_file
        .as_ref()
        .map(|path| save_file::SaveFile::open(path, &options.path, options.save_device))
        .transpose()?;
    if let Some(save) = save_file.as_ref() {
        writeln!(writer, "Save file: {}; writes only on clean exit, with unique backups when replacing existing data.", if save.loaded() { "loaded" } else { "new erased image" })?;
    } else if options.save_device != SaveDevice::None {
        writeln!(
            writer,
            "Save storage is volatile; use --save-file PATH for persistence."
        )?;
    }
    let steps = match options.mode {
        Mode::Terminal { steps } => steps,
        Mode::Window { frames } => {
            return crate::desktop::run_rom(
                bytes,
                frames,
                options.hardware,
                options.save_device,
                crate::desktop::RomPlayback {
                    audio: options.audio,
                    speed: options.speed,
                },
                save_file.as_mut(),
                writer,
            )
        }
    };
    writeln!(writer, "Machine-step limit: {steps} (includes boot)")?;
    let mut machine = bios::boot(bytes)?;
    machine
        .memory_mut()
        .set_cartridge_hardware(options.hardware);
    machine.memory_mut().set_save_device(options.save_device);
    if let Some(save) = save_file.as_ref() {
        save.initialize(machine.memory_mut())?;
    }
    let mut clock = if options.hardware == CartridgeHardware::Rtc {
        Some(rtc_clock::RtcHostClock::new(machine.memory_mut())?)
    } else {
        None
    };
    let report = run_bounded_with_clock(&mut machine, steps, clock.as_mut())?;
    write_report(writer, &machine, &report)?;
    writer.flush()?;
    if let Reason::Diagnostic(error) = report.reason {
        return Err(Box::new(error));
    }
    if let Some(save) = save_file.as_mut() {
        persist_save(save, machine.memory(), writer)?;
    }
    Ok(())
}

pub(crate) fn persist_save(
    save: &mut save_file::SaveFile,
    memory: &gba_core::memory::Memory,
    writer: &mut impl Write,
) -> io::Result<()> {
    if let Some(saved) = save.persist(memory)? {
        writeln!(writer, "Save persisted on clean exit.")?;
        if let Some(backup) = saved.backup {
            writeln!(writer, "Previous save backup: {backup:?}")?;
        }
    } else {
        writeln!(writer, "Save unchanged; no file written.")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
