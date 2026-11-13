//! Instruction lockstep against mGBA. Both sides report each executed instruction
//! outside the BIOS (0x0000-0x3fff) with the registers and cycle count right after
//! it. BIOS internals are skipped because the two BIOS implementations differ.

use std::{
    collections::VecDeque,
    io::{self, Write},
};

use gba_core::machine::{Machine, StepKind};

use crate::reference::Reference;

const BIOS_END: u32 = 0x4000;
/// Machine steps allowed while searching for the next instruction outside the BIOS.
const STEP_GUARD: usize = 50_000_000;
const BATCH: usize = 4096;
const CONTEXT: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub address: u32,
    pub thumb: bool,
    pub cycles: u64,
    pub gprs: [u32; 16],
    pub cpsr: u32,
}

impl Record {
    pub fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 20 {
            return None;
        }
        let hex = |text: &str| u32::from_str_radix(text, 16).ok();
        let mut gprs = [0; 16];
        for (slot, text) in gprs.iter_mut().zip(&fields[3..19]) {
            *slot = hex(text)?;
        }
        Some(Self {
            address: hex(fields[0])?,
            thumb: fields[1] == "1",
            cycles: fields[2].parse().ok()?,
            gprs,
            cpsr: hex(fields[19])?,
        })
    }
}

/// The next instruction this emulator executes outside the BIOS. An instruction that
/// enters the BIOS (SWI) is reported once control leaves it, because mGBA runs most
/// BIOS calls inside the SWI instruction itself.
pub fn next_local(machine: &mut Machine) -> Result<Record, String> {
    let step = |machine: &mut Machine| {
        let address = machine.cpu().pc();
        machine
            .step()
            .map_err(|error| format!("this emulator stopped at {address:#010x}: {error:?}"))
    };
    for _ in 0..STEP_GUARD {
        let address = machine.cpu().pc();
        let thumb = machine.cpu().cpsr() & 0x20 != 0;
        let kind = step(machine)?;
        if kind == StepKind::Instruction && address >= BIOS_END {
            let mut inside = 0;
            while machine.cpu().pc() < BIOS_END {
                inside += 1;
                if inside > STEP_GUARD {
                    return Err(format!("BIOS call at {address:#010x} did not return"));
                }
                step(machine)?;
            }
            let cpu = machine.cpu();
            return Ok(Record {
                address,
                thumb,
                cycles: machine.cycles(),
                gprs: *cpu.registers(),
                cpsr: cpu.cpsr(),
            });
        }
    }
    Err(format!(
        "no instruction outside the BIOS within {STEP_GUARD} steps"
    ))
}

/// Our r15 holds the next instruction address; mGBA's holds the pipelined fetch
/// address (next + 4 in ARM, next + 2 in Thumb). Compare architectural next PCs.
fn next_pc(record: &Record, mgba: bool) -> u32 {
    if mgba {
        record.gprs[15].wrapping_sub(if record.cpsr & 0x20 != 0 { 2 } else { 4 })
    } else {
        record.gprs[15]
    }
}

fn state_difference(ours: &Record, theirs: &Record) -> Option<String> {
    if ours.address != theirs.address || ours.thumb != theirs.thumb {
        return Some("executed different instructions".to_owned());
    }
    let mut differences = Vec::new();
    for register in 0..15 {
        if ours.gprs[register] != theirs.gprs[register] {
            differences.push(format!(
                "r{register} ours {:08x} mGBA {:08x}",
                ours.gprs[register], theirs.gprs[register]
            ));
        }
    }
    let (ours_pc, theirs_pc) = (next_pc(ours, false), next_pc(theirs, true));
    if ours_pc != theirs_pc {
        differences.push(format!("next PC ours {ours_pc:08x} mGBA {theirs_pc:08x}"));
    }
    if ours.cpsr != theirs.cpsr {
        differences.push(format!(
            "CPSR ours {:08x} mGBA {:08x}",
            ours.cpsr, theirs.cpsr
        ));
    }
    (!differences.is_empty()).then(|| differences.join(", "))
}

fn line(label: &str, record: &Record, delta: Option<u64>, opcode: Option<u32>) -> String {
    let delta = delta.map_or("     ".to_owned(), |delta| format!("{delta:+5}"));
    let opcode = opcode.map_or("        ".to_owned(), |word| {
        if record.thumb {
            format!("    {:04x}", word & 0xffff)
        } else {
            format!("{word:08x}")
        }
    });
    let gprs: Vec<String> = record.gprs[..13]
        .iter()
        .map(|value| format!("{value:08x}"))
        .collect();
    format!(
        "{label:5} {:08x} {opcode} {} cyc{delta} cpsr={:08x} sp={:08x} lr={:08x} r0-r12: {}",
        record.address,
        if record.thumb { "T" } else { "A" },
        record.cpsr,
        record.gprs[13],
        record.gprs[14],
        gprs.join(" ")
    )
}

fn opcode(machine: &Machine, record: &Record) -> Option<u32> {
    let memory = machine.memory();
    if record.thumb {
        memory.read16(record.address).ok().map(u32::from)
    } else {
        memory.read32(record.address).ok()
    }
}

/// Cycle drift: how far this emulator's clock is ahead of mGBA's since the start.
/// Per-instruction costs often differ only in attribution (a store charged here,
/// the following fetch there) and cancel at once. A drift is reported when the
/// offset settles at a new value, or has moved `RUNNING` cycles since the last report.
struct Drift {
    base: Option<(u64, u64)>,
    settled: i64,
    last: i64,
    stable: usize,
    window: VecDeque<(Record, i64)>,
    events: u64,
}

/// Records an offset must hold before a change counts as lasting.
const SETTLE: usize = 4;
/// A steadily drifting loop is reported once it has moved this many cycles.
const RUNNING: i64 = 64;
const WINDOW: usize = 12;

impl Drift {
    fn offset(&mut self, ours: &Record, theirs: &Record) -> i64 {
        let (ours_base, theirs_base) = *self.base.get_or_insert((ours.cycles, theirs.cycles));
        (ours.cycles - ours_base) as i64 - (theirs.cycles - theirs_base) as i64
    }
}

fn dump_history(
    out: &mut impl Write,
    machine: &Machine,
    history: &VecDeque<(Record, Record, Option<u64>, Option<u64>)>,
) -> io::Result<()> {
    for (ours, theirs, ours_delta, theirs_delta) in history {
        let word = opcode(machine, ours);
        writeln!(out, "{}", line("ours", ours, *ours_delta, word))?;
        writeln!(out, "{}", line("mGBA", theirs, *theirs_delta, word))?;
    }
    Ok(())
}

/// Lockstep up to `count` instructions. Reports up to `max_timing` cycle drifts
/// and stops at the first register or control-flow divergence.
/// Returns whether the run finished without a state divergence.
pub fn run(
    machine: &mut Machine,
    reference: &mut Reference,
    count: u64,
    max_timing: usize,
    out: &mut impl Write,
) -> Result<bool, String> {
    let report = |error: io::Error| error.to_string();
    let mut history: VecDeque<(Record, Record, Option<u64>, Option<u64>)> = VecDeque::new();
    let mut previous: Option<(u64, u64)> = None;
    let mut drift = Drift {
        base: None,
        settled: 0,
        last: 0,
        stable: 0,
        window: VecDeque::new(),
        events: 0,
    };
    let mut executed = 0u64;
    let mut pending: VecDeque<Record> = VecDeque::new();
    while executed < count {
        if pending.is_empty() {
            let batch = BATCH.min((count - executed) as usize);
            pending.extend(reference.sync(batch).map_err(report)?);
        }
        let theirs = pending.pop_front().ok_or("mgba-ref returned no records")?;
        let ours = match next_local(machine) {
            Ok(record) => record,
            Err(message) => {
                writeln!(out, "after {executed} instructions: {message}").map_err(report)?;
                dump_history(out, machine, &history).map_err(report)?;
                writeln!(out, "mGBA next: {}", line("mGBA", &theirs, None, None))
                    .map_err(report)?;
                return Ok(false);
            }
        };
        executed += 1;
        let deltas = previous.map(|(ours_before, theirs_before)| {
            (ours.cycles - ours_before, theirs.cycles - theirs_before)
        });
        previous = Some((ours.cycles, theirs.cycles));
        history.push_back((
            ours.clone(),
            theirs.clone(),
            deltas.map(|d| d.0),
            deltas.map(|d| d.1),
        ));
        if history.len() > CONTEXT {
            history.pop_front();
        }
        let offset = drift.offset(&ours, &theirs);
        if let Some(difference) = state_difference(&ours, &theirs) {
            writeln!(
                out,
                "state diverged at instruction {executed}: {difference}"
            )
            .map_err(report)?;
            dump_history(out, machine, &history).map_err(report)?;
            writeln!(
                out,
                "{executed} instructions compared; {} cycle drifts reported; this emulator is {offset:+} cycles relative to mGBA",
                drift.events
            )
            .map_err(report)?;
            return Ok(false);
        }

        let step = deltas.map_or(0, |(o, t)| o as i64 - t as i64);
        drift.window.push_back((ours, step));
        if drift.window.len() > WINDOW {
            drift.window.pop_front();
        }
        drift.stable = if offset == drift.last {
            drift.stable + 1
        } else {
            0
        };
        drift.last = offset;
        let moved = offset - drift.settled;
        if moved != 0 && (drift.stable >= SETTLE || moved.abs() >= RUNNING) {
            drift.events += 1;
            if drift.events as usize <= max_timing {
                writeln!(
                    out,
                    "drift #{} by instruction {executed}: {moved:+} cycles (now {offset:+} relative to mGBA); per-instruction differences:",
                    drift.events
                )
                .map_err(report)?;
                for (record, step) in drift.window.iter().filter(|(_, step)| *step != 0) {
                    let word = opcode(machine, record);
                    writeln!(out, "  {step:+4} {}", line("ours", record, None, word))
                        .map_err(report)?;
                }
            }
            drift.settled = offset;
            drift.window.clear();
        }
    }
    writeln!(
        out,
        "{executed} instructions compared without a state divergence; {} cycle drifts reported; this emulator is {:+} cycles relative to mGBA",
        drift.events, drift.last
    )
    .map_err(report)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(address: u32, thumb: bool, next: u32, cpsr: u32) -> Record {
        let mut gprs = [0; 16];
        gprs[15] = next;
        Record {
            address,
            thumb,
            cycles: 0,
            gprs,
            cpsr,
        }
    }

    #[test]
    fn records_parse_the_shim_line_format() {
        let line = "08000000 0 20 00000001 00000000 00000000 00000000 00000000 00000000 \
                    00000000 00000000 00000000 00000000 00000000 00000000 00000000 \
                    03007f00 00000000 080000c4 0000001f";
        let parsed = Record::parse(line).unwrap();
        assert_eq!(parsed.address, 0x0800_0000);
        assert!(!parsed.thumb);
        assert_eq!(parsed.cycles, 20);
        assert_eq!(parsed.gprs[0], 1);
        assert_eq!(parsed.gprs[13], 0x0300_7f00);
        assert_eq!(parsed.cpsr, 0x1f);
        assert_eq!(Record::parse("stuck"), None);
        assert_eq!(Record::parse(&line.replace("0000001f", "xyz")), None);
    }

    #[test]
    fn pipelined_mgba_pc_is_compared_as_the_next_instruction() {
        let ours = record(0x0800_0000, false, 0x0800_00c0, 0x1f);
        let theirs = record(0x0800_0000, false, 0x0800_00c4, 0x1f);
        assert_eq!(state_difference(&ours, &theirs), None);
        let ours = record(0x0800_0100, true, 0x0800_0102, 0x3f);
        let theirs = record(0x0800_0100, true, 0x0800_0104, 0x3f);
        assert_eq!(state_difference(&ours, &theirs), None);
        let theirs = record(0x0800_0100, true, 0x0800_0106, 0x3f);
        assert!(state_difference(&ours, &theirs)
            .unwrap()
            .contains("next PC ours 08000102 mGBA 08000104"));
    }

    #[test]
    fn different_instructions_or_registers_are_divergences() {
        let ours = record(0x0800_0000, false, 0x0800_0004, 0x1f);
        let other = record(0x0300_0000, false, 0x0300_0008, 0x92);
        assert_eq!(
            state_difference(&ours, &other).as_deref(),
            Some("executed different instructions")
        );
        let mut theirs = record(0x0800_0000, false, 0x0800_0008, 0x1f);
        theirs.gprs[3] = 7;
        assert!(state_difference(&ours, &theirs)
            .unwrap()
            .contains("r3 ours 00000000 mGBA 00000007"));
    }
}
