use super::*;
use gba_core::{
    cpu::Cpu,
    dma::DMA_BASE,
    io::{HALTCNT, IE, IME, TIMER_BASE},
    memory::{Memory, ROM_START},
};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn boot(code: &[u32]) -> Machine {
    bios::boot(code.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

#[test]
fn options_require_both_flags_in_either_order() {
    for count in [1, MAX_STEPS] {
        let count_text = count.to_string();
        for arguments in [
            args(&["--rom", "original program.gba", "--steps", &count_text]),
            args(&["--steps", &count_text, "--rom", "original program.gba"]),
        ] {
            assert_eq!(
                parse_args(&arguments).unwrap(),
                Options {
                    path: "original program.gba".into(),
                    mode: Mode::Terminal { steps: count },
                    hardware: CartridgeHardware::None,
                    save_device: SaveDevice::None,
                    audio: false,
                }
            );
        }
    }
}

#[test]
fn audio_is_opt_in_window_only_and_cannot_be_duplicated() {
    assert!(
        parse_args(&args(&["--audio", "--rom", "test.gba", "--window"]))
            .unwrap()
            .audio
    );
    assert!(
        !parse_args(&args(&["--rom", "test.gba", "--window"]))
            .unwrap()
            .audio
    );
    for tail in [
        vec!["--audio", "--steps", "10"],
        vec!["--audio", "--window", "--audio"],
    ] {
        let mut arguments = args(&["--rom", "test.gba"]);
        arguments.extend(args(&tail));
        assert!(parse_args(&arguments).is_err());
    }
}

#[test]
fn window_options_accept_any_order_and_optional_frame_limits() {
    for arguments in [
        args(&["--rom", "original.gba", "--window"]),
        args(&["--window", "--rom", "original.gba"]),
    ] {
        assert_eq!(
            parse_args(&arguments).unwrap().mode,
            Mode::Window { frames: None }
        );
    }
    for count in [1, MAX_FRAMES] {
        for arguments in [
            args(&[
                "--rom",
                "original.gba",
                "--window",
                "--frames",
                &count.to_string(),
            ]),
            args(&[
                "--frames",
                &count.to_string(),
                "--window",
                "--rom",
                "original.gba",
            ]),
        ] {
            assert_eq!(
                parse_args(&arguments).unwrap().mode,
                Mode::Window {
                    frames: Some(count)
                }
            );
        }
    }
}

#[test]
fn window_options_reject_conflicting_modes_and_invalid_frame_limits() {
    for tail in [
        vec![],
        vec!["--window", "--window"],
        vec!["--frames", "1"],
        vec!["--window", "--steps", "1"],
        vec!["--steps", "1", "--frames", "1"],
        vec!["--window", "--frames"],
        vec!["--window", "--help"],
        vec!["--window", "--cpu-demo"],
        vec!["--window", "--frames", "1", "--frames", "2"],
    ] {
        let mut arguments = args(&["--rom", "original.gba"]);
        arguments.extend(args(&tail));
        assert!(parse_args(&arguments).is_err(), "{arguments:?}");
    }
    for count in ["", "0", "-1", "+1", " 1", "100001", "18446744073709551616"] {
        assert!(parse_args(&args(&[
            "--rom",
            "original.gba",
            "--window",
            "--frames",
            count
        ]))
        .is_err());
    }
    assert!(parse_args(&args(&["--window"])).is_err());
}

#[test]
fn options_reject_missing_duplicate_conflicting_and_invalid_values() {
    for arguments in [
        vec![],
        vec!["--rom"],
        vec!["--steps"],
        vec!["--rom", "test.gba"],
        vec!["--steps", "1"],
        vec!["--rom", "", "--steps", "1"],
        vec!["--rom", "test.gba", "--steps"],
        vec!["--rom", "test.gba", "--steps", "1", "--help"],
        vec!["--rom", "test.gba", "--steps", "1", "--cpu-demo", "1"],
        vec!["--rom", "test.gba", "--rom", "test2.gba", "--steps", "1"],
        vec!["--rom", "test.gba", "--steps", "1", "--steps", "2"],
        vec!["--unknown", "test.gba", "--steps", "1"],
    ] {
        assert!(parse_args(&args(&arguments)).is_err(), "{arguments:?}");
    }
    for count in [
        "",
        "0",
        "-1",
        "+1",
        " 1",
        "1 ",
        "1.0",
        "0x10",
        "１",
        "100000001",
        "18446744073709551616",
    ] {
        assert!(
            parse_args(&args(&["--rom", "test.gba", "--steps", count])).is_err(),
            "{count:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn options_preserve_non_utf8_paths_but_reject_non_utf8_counts() {
    use std::os::unix::ffi::OsStringExt;
    let raw = OsString::from_vec(b"original-\xff.gba".to_vec());
    let mut arguments = args(&["--rom", "unused", "--steps", "1"]);
    arguments[1] = raw.clone();
    assert_eq!(
        parse_args(&arguments).unwrap().path,
        PathBuf::from(raw.clone())
    );
    arguments[3] = raw;
    assert!(parse_args(&arguments).is_err());
}

#[test]
fn bounded_reader_accepts_exact_capacity_and_does_not_pad_odd_lengths() {
    for length in 4..=8 {
        let bytes = vec![0x42; length];
        assert_eq!(read_bounded(bytes.as_slice(), 8).unwrap(), bytes);
    }
    for length in [0, 1, 2, 3, 9] {
        assert_eq!(
            read_bounded(vec![0; length].as_slice(), 8)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn bounded_reader_consumes_at_most_capacity_plus_one() {
    let bytes = [0; 20];
    let mut reader = io::Cursor::new(bytes);
    assert!(read_bounded(&mut reader, 8).is_err());
    assert_eq!(reader.position(), 9);
}

#[test]
fn reader_handles_short_reads_and_propagates_errors() {
    struct ByteReader {
        remaining: usize,
    }
    impl Read for ByteReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.remaining == 0 || output.is_empty() {
                return Ok(0);
            }
            self.remaining -= 1;
            output[0] = 0x42;
            Ok(1)
        }
    }
    assert_eq!(
        read_bounded(ByteReader { remaining: 5 }, 8).unwrap(),
        [0x42; 5]
    );
    struct FailingReader;
    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }
    assert_eq!(
        read_bounded(FailingReader, 8).unwrap_err().to_string(),
        "read failed"
    );
}

#[test]
fn bounded_runner_includes_boot_and_retains_original_program_results() {
    let mut machine = boot(&[0xe3a0_002a, 0xeaff_fffe]); // MOV r0,#42; B .
    let first = run_bounded(&mut machine, 1);
    assert_eq!(first.reason, Reason::StepLimit);
    assert_eq!(first.stats.instructions, 1);
    assert!(machine.cpu().pc() < ROM_START);
    let report = run_bounded(&mut machine, 100);
    assert_eq!(report.reason, Reason::StepLimit);
    assert_eq!(report.stats.steps, 100);
    assert_eq!(report.stats.instructions, 100);
    assert_eq!(machine.cpu().registers()[0], 42);
    assert_eq!(machine.cpu().pc(), ROM_START + 4);
}

#[test]
fn original_program_can_switch_to_thumb() {
    let mut bytes: Vec<u8> = [0xe59f_1000u32, 0xe12f_ff11, ROM_START + 13]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    bytes.extend([0x2a, 0x20, 0xfe, 0xe7]); // MOV r0,#42; B . in Thumb.
    let mut machine = bios::boot(bytes).unwrap();
    let report = run_bounded(&mut machine, 100);
    assert_eq!(report.reason, Reason::StepLimit);
    assert_eq!(
        machine.cpu().instruction_set(),
        gba_core::cpu::InstructionSet::Thumb
    );
    assert_eq!(machine.cpu().registers()[0], 42);
    assert_eq!(machine.cpu().pc(), ROM_START + 14);
}

#[test]
fn diagnostics_keep_the_failed_pc_registers_and_clock() {
    for code in [vec![0xee00_0000], vec![0xe3a0_002a]] {
        let mut machine = boot(&code);
        let report = run_bounded(&mut machine, 100);
        assert!(matches!(report.reason, Reason::Diagnostic(_)));
        assert!(report.stats.steps < 100);
        assert_eq!(
            machine.cpu().pc(),
            ROM_START + if code[0] == 0xee00_0000 { 0 } else { 4 }
        );
        let cpu = machine.cpu().clone();
        let cycles = machine.cycles();
        let retry = run_bounded(&mut machine, 100);
        assert_eq!(retry.stats.steps, 0);
        assert_eq!(retry.reason, report.reason);
        assert_eq!(machine.cpu(), &cpu);
        assert_eq!(machine.cycles(), cycles);
    }
}

#[test]
fn stop_finishes_promptly_even_at_the_exact_budget_boundary() {
    let code = [0xef03_0000, 0xeaff_fffe]; // SWI Stop; B .
    let mut machine = boot(&code);
    let report = run_bounded(&mut machine, 1000);
    assert_eq!(report.reason, Reason::Stopped);
    assert!(report.stats.steps > 0 && report.stats.steps < 1000);
    let cycles = machine.cycles();
    let retry = run_bounded(&mut machine, 1000);
    assert_eq!(retry.reason, Reason::Stopped);
    assert_eq!(retry.stats.steps, 0);
    assert_eq!(machine.cycles(), cycles);
    let mut exact = boot(&code);
    assert_eq!(
        run_bounded(&mut exact, report.stats.steps).reason,
        Reason::Stopped
    );
}

#[test]
fn halt_idles_consume_the_budget() {
    let mut machine = boot(&[0xef02_0000, 0xeaff_fffe]);
    let report = run_bounded(&mut machine, 1000);
    assert_eq!(report.reason, Reason::StepLimit);
    assert_eq!(report.stats.steps, 1000);
    assert!(report.stats.halt_idle > 0);
    assert_eq!(report.stats.instructions + report.stats.halt_idle, 1000);
    assert!(machine.halted());
    assert!(!machine.stopped());
}

#[test]
fn dma_units_and_irq_entries_each_consume_one_step() {
    let mut machine = boot(&[0xeaff_fffe]);
    run_bounded(&mut machine, 100);
    let pc = machine.cpu().pc();
    let bus = machine.memory_mut();
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    bus.write32(DMA_BASE + 4, 0x0300_0000).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0002).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    let report = run_bounded(&mut machine, 2);
    assert_eq!(report.stats.dma_units, 2);
    assert_eq!(report.stats.instructions, 0);
    assert_eq!(machine.cpu().pc(), pc);
    let report = run_bounded(&mut machine, 1);
    assert_eq!(report.stats.irq_entries, 1);
    assert_eq!(report.stats.steps, 1);
    assert_eq!(machine.cpu().pc(), 0x18);
}

#[test]
fn dma_errors_return_a_diagnostic_without_consuming_a_step() {
    let mut machine = boot(&[0xeaff_fffe]);
    let bus = machine.memory_mut();
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    // DMA0 masks cartridge addresses to BIOS; use an actually unmapped destination.
    bus.write32(DMA_BASE + 4, 0x0000_4000).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0001).unwrap();
    let report = run_bounded(&mut machine, 100);
    assert!(matches!(
        report.reason,
        Reason::Diagnostic(MachineError::Dma(_))
    ));
    assert_eq!(report.stats.steps, 0);
    assert_eq!(machine.cycles(), 0);
}

#[test]
fn report_shows_state_and_propagates_write_errors() {
    let mut machine = Machine::new(Cpu::new(ROM_START), Memory::new(vec![]).unwrap());
    machine.memory_mut().write8(HALTCNT, 0x80).unwrap();
    let report = run_bounded(&mut machine, 100);
    let mut output = Vec::new();
    write_report(&mut output, &machine, &report).unwrap();
    let output = String::from_utf8(output).unwrap();
    for expected in [
        "Result: STOP",
        "Steps: 0",
        "Nominal cycles: 0",
        "PC=0x08000000",
        "halted=false stopped=true",
        "r15=0x08000000",
    ] {
        assert!(output.contains(expected), "{output}");
    }
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        write_report(&mut FailingWriter, &machine, &report)
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}
