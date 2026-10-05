//! Host-only raw ROM loading and bounded terminal diagnostics.

use std::{
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use gba_core::{
    bios,
    machine::{Machine, MachineError, StepKind},
    memory::ROM_CAPACITY,
};

const MAX_STEPS: u64 = 100_000_000;
const USAGE: &str = "usage: gameboy-rust --rom PATH --steps COUNT (1..=100000000); do not combine with demo or help options";

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    path: PathBuf,
    steps: u64,
}

pub fn parse_args(args: &[OsString]) -> io::Result<Options> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, USAGE);
    let mut path = None;
    let mut steps = None;
    let mut pairs = args.chunks_exact(2);
    for pair in &mut pairs {
        if pair[0] == "--rom" && path.is_none() && !pair[1].is_empty() {
            path = Some(PathBuf::from(&pair[1]));
        } else if pair[0] == "--steps" && steps.is_none() {
            let value = pair[1].to_str().ok_or_else(invalid)?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            let count = value.parse::<u64>().map_err(|_| invalid())?;
            if !(1..=MAX_STEPS).contains(&count) {
                return Err(invalid());
            }
            steps = Some(count);
        } else {
            return Err(invalid());
        }
    }
    if !pairs.remainder().is_empty() {
        return Err(invalid());
    }
    Ok(Options {
        path: path.ok_or_else(invalid)?,
        steps: steps.ok_or_else(invalid)?,
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

fn run_bounded(machine: &mut Machine, limit: u64) -> Report {
    let mut stats = Stats::default();
    let reason = loop {
        // STOP has no host input source here. Do not spin with a frozen clock.
        // Check before the budget so STOP on the last step is reported correctly.
        if machine.stopped() {
            break Reason::Stopped;
        }
        if stats.steps == limit {
            break Reason::StepLimit;
        }
        match machine.step() {
            Ok(StepKind::Instruction) => stats.instructions += 1,
            Ok(StepKind::IrqEntry) => stats.irq_entries += 1,
            Ok(StepKind::Dma { .. }) => stats.dma_units += 1,
            Ok(StepKind::HaltIdle) => stats.halt_idle += 1,
            Ok(StepKind::StopIdle) => break Reason::Stopped,
            Err(error) => break Reason::Diagnostic(error),
        }
        stats.steps += 1;
    };
    Report { stats, reason }
}

fn write_report(writer: &mut impl Write, machine: &Machine, report: &Report) -> io::Result<()> {
    let reason = match report.reason {
        Reason::StepLimit => "step limit reached",
        Reason::Stopped => "STOP; input required (no input source in terminal mode)",
        Reason::Diagnostic(_) => "emulation diagnostic",
    };
    writeln!(writer, "Result: {reason}")?;
    let stats = &report.stats;
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
    writeln!(
        writer,
        "Machine-step limit: {} (includes boot)",
        options.steps
    )?;
    let mut machine = bios::boot(bytes)?;
    let report = run_bounded(&mut machine, options.steps);
    write_report(writer, &machine, &report)?;
    writer.flush()?;
    if let Reason::Diagnostic(error) = report.reason {
        return Err(Box::new(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
