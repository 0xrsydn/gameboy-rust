use super::*;
use gba_core::{
    dma::DMA_BASE,
    io::{IE, IME, TIMER_BASE},
    memory::ROM_START,
};

fn case_value() -> Value {
    json!({"name": "original", "rom": "original.gba", "step_limit": 100,
        "completion": {"pc": "0x08000004", "instruction_set": "arm"},
        "checks": [{"kind": "register", "index": 0, "equals": "0x2a"}]})
}
fn case() -> Case {
    serde_json::from_value(case_value()).unwrap()
}
pub(super) fn suite(value: Value) -> Result<Suite, Box<dyn Error>> {
    let suite: Suite = serde_json::from_value(value)?;
    suite.validate()?;
    Ok(suite)
}
pub(super) fn boot(words: &[u32]) -> Machine {
    bios::boot(words.iter().flat_map(|word| word.to_le_bytes()).collect()).unwrap()
}

#[test]
fn suite_requires_one_path_and_no_other_options() {
    for args in [
        vec![],
        vec!["--test-suite"],
        vec!["--test-suite", ""],
        vec!["--test-suite", "a.json", "--help"],
        vec!["--rom", "a.json"],
    ] {
        assert!(parse_args(&args.into_iter().map(OsString::from).collect::<Vec<_>>()).is_err());
    }
    assert_eq!(
        parse_args(&["--test-suite".into(), "space λ.json".into()]).unwrap(),
        PathBuf::from("space λ.json")
    );
}

#[test]
fn word_values_accept_u32_numbers_decimal_strings_and_hex() {
    for value in [
        json!(42),
        json!("42"),
        json!("0x2a"),
        json!("0x2A"),
        json!(4294967295u32),
        json!("0xffffffff"),
    ] {
        let mut input = case_value();
        input["checks"][0]["equals"] = value;
        assert!(serde_json::from_value::<Case>(input).is_ok());
    }
    for value in [
        json!(-1),
        json!(42.0),
        json!(4294967296u64),
        json!(null),
        json!(true),
        json!(""),
        json!("-1"),
        json!("+1"),
        json!(" 1"),
        json!("1 "),
        json!("0x"),
        json!("0x100000000"),
        json!("0xgg"),
        json!("1.0"),
    ] {
        let mut input = case_value();
        input["checks"][0]["equals"] = value.clone();
        assert!(serde_json::from_value::<Case>(input).is_err(), "{value}");
    }
}

#[test]
fn unknown_fields_and_unknown_check_kinds_are_rejected() {
    for pointer in ["", "/cases/0", "/cases/0/completion", "/cases/0/checks/0"] {
        let mut value = json!({"version": 1, "cases": [case_value()]});
        value.pointer_mut(pointer).unwrap()["typo"] = json!(1);
        assert!(suite(value).is_err());
    }
    let mut value = case_value();
    value["checks"][0]["kind"] = json!("memory8");
    assert!(suite(json!({"version": 1, "cases": [value]})).is_err());
    assert!(serde_json::from_str::<Suite>(r#"{"version":1,"version":1,"cases":[]}"#).is_err());
}

#[test]
fn manifest_rejects_empty_duplicate_excessive_or_invalid_cases() {
    assert!(suite(json!({"version": 3, "cases": [case_value()]})).is_err());
    assert!(suite(json!({"version": 1, "cases": []})).is_err());
    assert!(suite(json!({"version": 1, "cases": [case_value(), case_value()]})).is_err());
    for (field, invalid) in [
        ("name", json!(" ")),
        ("name", json!("x".repeat(129))),
        ("rom", json!("")),
        ("step_limit", json!(0)),
        ("step_limit", json!(100000001u64)),
        ("checks", json!([])),
    ] {
        let mut value = case_value();
        value[field] = invalid;
        assert!(suite(json!({"version": 1, "cases": [value]})).is_err());
    }
    let cases: Vec<_> = (0..257)
        .map(|index| {
            let mut value = case_value();
            value["name"] = json!(format!("test-{index}"));
            value
        })
        .collect();
    assert!(suite(json!({"version": 1, "cases": cases})).is_err());
    let mut first = case_value();
    first["step_limit"] = json!(MAX_STEPS);
    assert!(suite(json!({"version": 1, "cases": [first.clone()]})).is_ok());
    let mut second = case_value();
    second["name"] = json!("second");
    assert!(suite(json!({"version": 1, "cases": [first, second]})).is_err());
}

#[test]
fn manifest_checks_alignment_register_bounds_and_check_count() {
    for (pc, state, valid) in [
        ("0x08000002", "arm", false),
        ("0x08000002", "thumb", true),
        ("0x08000003", "thumb", false),
        ("0x08000004", "invalid", false),
    ] {
        let mut value = case_value();
        value["completion"] = json!({"pc": pc, "instruction_set": state});
        assert_eq!(
            suite(json!({"version": 1, "cases": [value]})).is_ok(),
            valid
        );
    }
    for check in [
        json!({"kind": "register", "index":16,"equals":0}),
        json!({"kind":"memory32","address":"0x02000001","equals":0}),
    ] {
        let mut value = case_value();
        value["checks"] = json!([check]);
        assert!(suite(json!({"version": 1, "cases": [value]})).is_err());
    }
    let mut value = case_value();
    value["checks"] = json!(vec![value["checks"][0].clone(); MAX_CHECKS + 1]);
    assert!(suite(json!({"version": 1, "cases": [value]})).is_err());
}

#[test]
fn json_report_propagates_write_and_flush_failures() {
    struct Fails {
        on_flush: bool,
    }
    impl Write for Fails {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.on_flush {
                Ok(bytes.len())
            } else {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failure"))
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failure"))
        }
    }
    let report = json!({"passed":true});
    for on_flush in [false, true] {
        let error = write_json_report(&mut Fails { on_flush }, &report).unwrap_err();
        assert!(error.to_string().contains(if on_flush {
            "flush failure"
        } else {
            "write failure"
        }));
    }
    let mut output = Vec::new();
    write_json_report(&mut output, &report).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&output).unwrap(), report);
    assert_eq!(output.last(), Some(&b'\n'));
}

#[test]
fn reset_pc_is_not_an_immediate_pass_without_execution() {
    let mut config = case();
    config.completion = Completion::Pc {
        pc: 0,
        instruction_set: State::Arm,
    };
    config.step_limit = 1;
    let (stats, outcome) = run_to_checkpoint(&mut boot(&[0xeaff_fffe]), &config);
    assert_eq!(stats.steps, 1);
    assert_eq!(outcome, Outcome::StepLimit);
}

#[test]
fn checkpoint_is_checked_before_execution_and_on_the_last_budgeted_step() {
    let code = [0xe3a0_002a, 0xee00_0000]; // MOV r0,#42; unsupported (never fetched)
    let mut machine = boot(&code);
    let (stats, outcome) = run_to_checkpoint(&mut machine, &case());
    assert_eq!(outcome, Outcome::Checkpoint);
    assert!(stats.steps > 1); // BIOS boot is included.
    assert_eq!(check(&machine, &case().checks[0])["passed"], true);
    let mut exact = case();
    exact.step_limit = stats.steps;
    assert_eq!(
        run_to_checkpoint(&mut boot(&code), &exact).1,
        Outcome::Checkpoint
    );
    exact.step_limit -= 1;
    assert_eq!(
        run_to_checkpoint(&mut boot(&code), &exact).1,
        Outcome::StepLimit
    );
}

#[test]
fn matching_assertions_do_not_pass_without_the_checkpoint() {
    let mut config = case();
    config.completion = Completion::Pc {
        pc: ROM_START + 8,
        instruction_set: State::Arm,
    };
    let mut machine = boot(&[0xe3a0_002a, 0xeaff_fffe]);
    let (stats, outcome) = run_to_checkpoint(&mut machine, &config);
    assert_eq!(outcome, Outcome::StepLimit);
    assert_eq!(stats.steps, config.step_limit);
    assert_eq!(check(&machine, &config.checks[0])["passed"], true);
    config.completion = Completion::Pc {
        pc: ROM_START + 4,
        instruction_set: State::Thumb,
    };
    assert_eq!(
        run_to_checkpoint(&mut boot(&[0xe3a0_002a, 0xeaff_fffe]), &config).1,
        Outcome::StepLimit
    );
}

#[test]
fn thumb_checkpoint_uses_aligned_architectural_pc() {
    let mut bytes: Vec<u8> = [0xe59f_1000u32, 0xe12f_ff11, ROM_START + 13]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    bytes.extend([0x2a, 0x20, 0xfe, 0xe7]);
    let mut machine = bios::boot(bytes).unwrap();
    let mut config = case();
    config.completion = Completion::Pc {
        pc: ROM_START + 14,
        instruction_set: State::Thumb,
    };
    assert_eq!(
        run_to_checkpoint(&mut machine, &config).1,
        Outcome::Checkpoint
    );
    assert_eq!(check(&machine, &config.checks[0])["passed"], true);
}

#[test]
fn memory_and_cpsr_checks_report_actual_values_and_read_errors() {
    let mut config = case();
    config.completion = Completion::Pc {
        pc: ROM_START + 12,
        instruction_set: State::Arm,
    };
    let mut machine = boot(&[0xe3a0_002a, 0xe3a0_1402, 0xe581_0000, 0xeaff_fffe]);
    assert_eq!(
        run_to_checkpoint(&mut machine, &config).1,
        Outcome::Checkpoint
    );
    for assertion in [
        Check::Memory32 {
            address: 0x0200_0000,
            equals: 42,
        },
        Check::Cpsr { equals: 0x1f },
    ] {
        let result = check(&machine, &assertion);
        assert_eq!(result["passed"], true);
        assert!(result["error"].is_null());
    }
    let wrong = check(
        &machine,
        &Check::Register {
            index: 0,
            equals: 43,
        },
    );
    assert_eq!(wrong["passed"], false);
    assert_eq!(wrong["actual"], 42);
    let unmapped = check(
        &machine,
        &Check::Memory32 {
            address: 0x0100_0000,
            equals: 0,
        },
    );
    assert_eq!(unmapped["passed"], false);
    assert!(unmapped["actual"].is_null());
    assert!(unmapped["error"].is_string());
}

#[test]
fn stop_is_failure_while_halt_consumes_a_bounded_budget() {
    let mut config = case();
    config.completion = Completion::Pc {
        pc: ROM_START + 8,
        instruction_set: State::Arm,
    };
    let (stats, outcome) = run_to_checkpoint(&mut boot(&[0xef03_0000, 0xeaff_fffe]), &config);
    assert_eq!(outcome, Outcome::Stopped);
    assert!(stats.steps < config.step_limit);
    let (stats, outcome) = run_to_checkpoint(&mut boot(&[0xef02_0000, 0xeaff_fffe]), &config);
    assert_eq!(outcome, Outcome::StepLimit);
    assert_eq!(stats.steps, config.step_limit);
    assert!(stats.halt_idle > 0);
}

#[test]
fn failing_instructions_do_not_advance_the_reported_state() {
    let mut machine = boot(&[0xee00_0000]);
    let (stats, outcome) = run_to_checkpoint(&mut machine, &case());
    assert!(matches!(outcome, Outcome::EmulationError(_)));
    assert!(stats.steps < case().step_limit);
    assert_eq!(machine.cpu().pc(), ROM_START);
    let cpu = machine.cpu().clone();
    let cycles = machine.cycles();
    let (retry, outcome) = run_to_checkpoint(&mut machine, &case());
    assert_eq!(retry.steps, 0);
    assert!(matches!(outcome, Outcome::EmulationError(_)));
    assert_eq!(machine.cpu(), &cpu);
    assert_eq!(machine.cycles(), cycles);
}

#[test]
fn dma_and_irq_each_consume_a_budget_step_and_dma_errors_fail() {
    let mut machine = boot(&[0xe3a0_002a, 0xeaff_fffe]);
    run_to_checkpoint(&mut machine, &case());
    let mut config = case();
    config.completion = Completion::Pc {
        pc: ROM_START + 8,
        instruction_set: State::Arm,
    };
    config.step_limit = 3;
    let bus = machine.memory_mut();
    bus.write32(DMA_BASE, 0x0200_0000).unwrap();
    bus.write32(DMA_BASE + 4, 0x0300_0000).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0002).unwrap();
    bus.write16(IE, 8).unwrap();
    bus.write16(IME, 1).unwrap();
    bus.write32(TIMER_BASE, 0x00c0_ffff).unwrap();
    bus.advance_cycles(1);
    let (stats, outcome) = run_to_checkpoint(&mut machine, &config);
    assert_eq!(outcome, Outcome::StepLimit);
    assert_eq!(stats.dma_units, 2);
    assert_eq!(stats.irq_entries, 1);
    assert_eq!(stats.instructions, 0);
    let bus = machine.memory_mut();
    bus.write32(DMA_BASE + 4, ROM_START).unwrap();
    bus.write32(DMA_BASE + 8, 0x8000_0001).unwrap();
    let (stats, outcome) = run_to_checkpoint(&mut machine, &config);
    assert_eq!(stats.steps, 0);
    assert!(matches!(outcome, Outcome::EmulationError(_)));
}
