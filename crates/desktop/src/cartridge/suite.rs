//! Bounded, headless ROM tests with explicit checkpoints and assertions.
//! Passing means only that the configured checkpoint and checks succeeded.

use std::{
    collections::HashSet,
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use gba_core::{
    bios,
    cpu::InstructionSet,
    machine::{Machine, StepKind},
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use super::{load, Stats, MAX_STEPS};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CASES: usize = 256;
const MAX_CHECKS: usize = 256;
const USAGE: &str = "usage: gameboy-rust --test-suite PATH.json (no other options)";

pub fn parse_args(args: &[OsString]) -> io::Result<PathBuf> {
    match args {
        [flag, path] if flag == "--test-suite" && !path.is_empty() => Ok(path.into()),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE)),
    }
}

/// JSON integers or unsigned decimal/0x-prefixed strings, never floats or signs.
fn word<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let parsed = match value {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => {
            let (digits, radix) = text
                .strip_prefix("0x")
                .map_or((text.as_str(), 10), |digits| (digits, 16));
            if digits.is_empty()
                || !digits.bytes().all(|byte| {
                    if radix == 16 {
                        byte.is_ascii_hexdigit()
                    } else {
                        byte.is_ascii_digit()
                    }
                })
            {
                None
            } else {
                u32::from_str_radix(digits, radix).ok()
            }
        }
        _ => None,
    };
    parsed.ok_or_else(|| {
        serde::de::Error::custom("expected a u32 integer or unsigned decimal/0x string")
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    version: u32,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    rom: PathBuf,
    step_limit: u64,
    completion: Completion,
    checks: Vec<Check>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    #[serde(deserialize_with = "word")]
    pc: u32,
    instruction_set: State,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Arm,
    Thumb,
}

impl State {
    fn instruction_set(self) -> InstructionSet {
        match self {
            Self::Arm => InstructionSet::Arm,
            Self::Thumb => InstructionSet::Thumb,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Check {
    Register {
        index: usize,
        #[serde(deserialize_with = "word")]
        equals: u32,
    },
    Memory32 {
        #[serde(deserialize_with = "word")]
        address: u32,
        #[serde(deserialize_with = "word")]
        equals: u32,
    },
    Cpsr {
        #[serde(deserialize_with = "word")]
        equals: u32,
    },
}

impl Suite {
    fn validate(&self) -> io::Result<()> {
        let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidInput, message);
        if self.version != 1 {
            return Err(invalid("suite version must be 1"));
        }
        if self.cases.is_empty() || self.cases.len() > MAX_CASES {
            return Err(invalid("suite must contain 1..=256 cases"));
        }
        let mut names = HashSet::new();
        let mut total = 0u64;
        for case in &self.cases {
            if case.name.trim().is_empty() || case.name.len() > 128 || !names.insert(&case.name) {
                return Err(invalid(
                    "case names must be nonempty, unique, and at most 128 UTF-8 bytes",
                ));
            }
            if case.rom.as_os_str().is_empty() {
                return Err(invalid("case ROM path must not be empty"));
            }
            if !(1..=MAX_STEPS).contains(&case.step_limit) {
                return Err(invalid("case step_limit must be 1..=100000000"));
            }
            total += case.step_limit;
            if total > MAX_STEPS {
                return Err(invalid("sum of case step limits must not exceed 100000000"));
            }
            let alignment = match case.completion.instruction_set {
                State::Arm => 4,
                State::Thumb => 2,
            };
            if case.completion.pc % alignment != 0 {
                return Err(invalid("completion PC must be aligned for its instruction set; do not set the Thumb address bit"));
            }
            if case.checks.is_empty() || case.checks.len() > MAX_CHECKS {
                return Err(invalid("each case must contain 1..=256 checks"));
            }
            for check in &case.checks {
                match check {
                    Check::Register { index, .. } if *index > 15 => {
                        return Err(invalid("register index must be 0..=15"))
                    }
                    Check::Memory32 { address, .. } if address % 4 != 0 => {
                        return Err(invalid("memory32 address must be word-aligned"))
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn read_suite(path: &Path) -> Result<Suite, Box<dyn Error>> {
    let validate = |metadata: fs::Metadata| -> io::Result<()> {
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "suite must be a regular file no larger than 1 MiB",
            ));
        }
        Ok(())
    };
    validate(fs::metadata(path)?)?;
    let file = File::open(path)?;
    validate(file.metadata()?)?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "suite exceeds 1 MiB").into());
    }
    let suite: Suite = serde_json::from_slice(&bytes)?;
    suite.validate()?;
    Ok(suite)
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Checkpoint,
    StepLimit,
    Stopped,
    EmulationError(String),
}

fn run_to_checkpoint(machine: &mut Machine, case: &Case) -> (Stats, Outcome) {
    let mut stats = Stats::default();
    loop {
        if machine.stopped() {
            return (stats, Outcome::Stopped);
        }
        // Check after successful progress, before trying to execute the checkpoint.
        // The final budgeted step can reach it. No initial/reset-state pass is allowed.
        if stats.steps != 0
            && machine.cpu().pc() == case.completion.pc
            && machine.cpu().instruction_set() == case.completion.instruction_set.instruction_set()
        {
            return (stats, Outcome::Checkpoint);
        }
        if stats.steps == case.step_limit {
            return (stats, Outcome::StepLimit);
        }
        match machine.step() {
            Ok(StepKind::StopIdle) => return (stats, Outcome::Stopped),
            Ok(kind) => stats.record(kind),
            Err(error) => return (stats, Outcome::EmulationError(error.to_string())),
        }
    }
}

fn check(machine: &Machine, check: &Check) -> Value {
    let (expected, actual) = match check {
        Check::Register { index, equals } => (*equals, Ok(machine.cpu().registers()[*index])),
        Check::Memory32 { address, equals } => (*equals, machine.memory().read32(*address)),
        Check::Cpsr { equals } => (*equals, Ok(machine.cpu().cpsr())),
    };
    match actual {
        Ok(actual) => {
            json!({"check": check, "actual": actual, "passed": actual == expected, "error": null})
        }
        Err(error) => {
            json!({"check": check, "actual": null, "passed": false, "error": error.to_string()})
        }
    }
}

fn state(machine: &Machine, stats: &Stats) -> Value {
    json!({
        "registers": machine.cpu().registers(), "pc": machine.cpu().pc(), "cpsr": machine.cpu().cpsr(),
        "mode": format!("{:?}", machine.cpu().mode()),
        "instruction_set": match machine.cpu().instruction_set() { InstructionSet::Arm => "arm", InstructionSet::Thumb => "thumb" },
        "halted": machine.halted(), "stopped": machine.stopped(), "nominal_cycles": machine.cycles(),
        "steps": stats.steps, "instructions": stats.instructions, "irq_entries": stats.irq_entries,
        "dma_units": stats.dma_units, "halt_idle": stats.halt_idle,
    })
}

fn run_case(case: &Case, base: &Path) -> Value {
    let path = base.join(&case.rom);
    // Debug formatting preserves non-UTF-8 host paths without lossy replacement.
    let mut report = json!({
        "name": case.name, "rom": case.rom, "resolved_path_debug": format!("{path:?}"),
        "step_limit": case.step_limit, "completion": case.completion,
        "passed": false, "reason": "load_error", "error": null,
        "rom_bytes": null, "state": null, "checks": [],
    });
    let bytes = match load(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            report["error"] = json!(error.to_string());
            return report;
        }
    };
    report["rom_bytes"] = json!(bytes.len());
    let mut machine = match bios::boot(bytes) {
        Ok(machine) => machine,
        Err(error) => {
            report["error"] = json!(error.to_string());
            return report;
        }
    };
    let (stats, outcome) = run_to_checkpoint(&mut machine, case);
    report["state"] = state(&machine, &stats);
    let reason = match outcome {
        Outcome::Checkpoint => {
            let checks: Vec<_> = case
                .checks
                .iter()
                .map(|assertion| check(&machine, assertion))
                .collect();
            let passed = checks.iter().all(|check| check["passed"] == true);
            report["passed"] = json!(passed);
            report["checks"] = json!(checks);
            if passed {
                "checkpoint"
            } else {
                "assertion_failed"
            }
        }
        Outcome::StepLimit => "step_limit",
        Outcome::Stopped => "stopped",
        Outcome::EmulationError(error) => {
            report["error"] = json!(error);
            "emulation_error"
        }
    };
    report["reason"] = json!(reason);
    report
}

fn write_json_report(writer: &mut impl Write, report: &Value) -> Result<(), Box<dyn Error>> {
    serde_json::to_writer_pretty(&mut *writer, report)?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

pub fn execute(path: &Path, writer: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let suite = read_suite(path)
        .map_err(|error| io::Error::other(format!("cannot read test suite {path:?}: {error}")))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let cases: Vec<_> = suite
        .cases
        .iter()
        .map(|case| run_case(case, base))
        .collect();
    let failed = cases.iter().filter(|case| case["passed"] != true).count();
    let report = json!({
        "format_version": 1, "bios": "original", "passed": failed == 0,
        "case_count": cases.len(), "failed_count": failed, "cases": cases,
    });
    write_json_report(writer, &report)?;
    if failed != 0 {
        return Err(io::Error::other(format!("ROM test suite: {failed} case(s) failed")).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
