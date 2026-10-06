//! File-backed suite tests. All ROM instructions are original and temporary.
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gba-suite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn rom(&self, name: &str, code: &[u32]) {
        fs::write(
            self.0.join(name),
            code.iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    fn manifest(&self, cases: Vec<Value>) -> PathBuf {
        let path = self.0.join("suite λ.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"version":1,"cases":cases})).unwrap(),
        )
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn case(name: &str, rom: &str) -> Value {
    json!({"name":name,"rom":rom,"step_limit":100,
        "completion":{"pc":"0x08000004","instruction_set":"arm"},
        "checks":[{"kind":"register","index":0,"equals":42}]})
}
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gameboy-rust"))
}
fn run(path: &Path) -> Output {
    command()
        .arg("--test-suite")
        .arg(path)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap()
}
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}
fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={:?}; stderr={}",
            output.stdout,
            stderr(output)
        )
    })
}

#[test]
fn relative_paths_resolve_from_suite_and_success_reports_are_repeatable() {
    let fixture = Fixture::new();
    fixture.rom("original λ.gba", &[0xe3a0_002a, 0xee00_0000]);
    let path = fixture.manifest(vec![case("original \"ARM\"\ncase", "original λ.gba")]);
    let first = run(&path);
    let second = run(&path);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let report = report(&first);
    assert_eq!(report["format_version"], 1);
    assert_eq!(report["bios"], "original");
    assert_eq!(report["passed"], true);
    assert_eq!(report["failed_count"], 0);
    let result = &report["cases"][0];
    assert_eq!(result["reason"], "checkpoint");
    assert_eq!(result["name"], "original \"ARM\"\ncase");
    assert_eq!(result["rom_bytes"], 8);
    assert_eq!(result["state"]["pc"], 0x0800_0004u32);
    assert_eq!(result["state"]["instruction_set"], "arm");
    assert_eq!(result["state"]["registers"].as_array().unwrap().len(), 16);
    assert_eq!(result["checks"][0]["actual"], 42);
}

#[test]
fn failures_do_not_skip_later_cases_and_never_count_as_passes() {
    let fixture = Fixture::new();
    fixture.rom("valid.gba", &[0xe3a0_002a, 0xeaff_fffe]);
    fixture.rom("invalid.gba", &[0xee00_0000]);
    fixture.rom("stop.gba", &[0xef03_0000, 0xeaff_fffe]);
    let mut mismatch = case("mismatch", "valid.gba");
    mismatch["checks"][0]["equals"] = json!(43);
    let mut timeout = case("timeout", "valid.gba");
    timeout["completion"]["pc"] = json!("0x08000008");
    let mut bad_read = case("read-error", "valid.gba");
    bad_read["checks"] = json!([{"kind":"memory32","address":"0x01000000","equals":0}]);
    let path = fixture.manifest(vec![
        mismatch,
        timeout,
        case("diagnostic", "invalid.gba"),
        case("stop", "stop.gba"),
        case("missing", "missing.gba"),
        bad_read,
        case("passes-last", "valid.gba"),
    ]);
    let output = run(&path);
    assert_eq!(output.status.code(), Some(1));
    let report = report(&output);
    assert_eq!(report["case_count"], 7);
    assert_eq!(report["failed_count"], 6);
    assert_eq!(report["passed"], false);
    for (index, reason) in [
        "assertion_failed",
        "step_limit",
        "emulation_error",
        "stopped",
        "load_error",
        "assertion_failed",
        "checkpoint",
    ]
    .iter()
    .enumerate()
    {
        let result = &report["cases"][index];
        assert_eq!(result["reason"], *reason);
        assert_eq!(result["passed"], index == 6);
    }
    assert_eq!(report["cases"][0]["checks"][0]["actual"], 42);
    assert_eq!(report["cases"][1]["state"]["steps"], 100);
    assert_eq!(report["cases"][1]["checks"], json!([]));
    assert_eq!(report["cases"][2]["state"]["pc"], 0x0800_0000u32);
    assert_eq!(report["cases"][3]["state"]["stopped"], true);
    assert!(report["cases"][4]["state"].is_null());
    assert!(report["cases"][5]["checks"][0]["error"].is_string());
    assert!(stderr(&output).contains("6 case(s) failed"));
}

#[test]
fn every_case_starts_with_fresh_machine_state() {
    let fixture = Fixture::new();
    fixture.rom(
        "write.gba",
        &[0xe3a0_002a, 0xe3a0_1402, 0xe581_0000, 0xeaff_fffe],
    );
    fixture.rom("fresh.gba", &[0xe3a0_002a, 0xeaff_fffe]);
    let mut writer = case("write", "write.gba");
    writer["completion"]["pc"] = json!("0x0800000c");
    writer["checks"] = json!([{"kind":"memory32","address":"0x02000000","equals":42}]);
    let mut fresh = case("fresh", "fresh.gba");
    fresh["checks"] = json!([{"kind":"memory32","address":"0x02000000","equals":0}, {"kind":"cpsr","equals":"0x1f"}]);
    let output = run(&fixture.manifest(vec![writer, fresh]));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(report(&output)["failed_count"], 0);
}

#[test]
fn malformed_or_oversized_suites_fail_before_any_case_execution() {
    let fixture = Fixture::new();
    let path = fixture.0.join("invalid.json");
    for bytes in [
        b"".as_slice(),
        b"{",
        br#"{"version":1,"cases":[]}"#,
        br#"{"version":2,"cases":[]}"#,
    ] {
        fs::write(&path, bytes).unwrap();
        let output = run(&path);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).contains("cannot read test suite"));
    }
    File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    let output = run(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("1 MiB"));
    for path in [&fixture.0, &fixture.0.join("missing.json")] {
        let output = run(path);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
    fixture.rom("valid.gba", &[0xe3a0_002a, 0xeaff_fffe]);
    let mut invalid = case("invalid-later", "valid.gba");
    invalid["checks"] = json!([]);
    let output = run(&fixture.manifest(vec![case("would-pass", "valid.gba"), invalid]));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn gameplay_reports_inputs_captured_pixels_and_failures_repeatably() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("input.gba"), gba_demos::input_rom()).unwrap();
    fixture.rom(
        "bad-video.gba",
        &[0xe3a0_1301, 0xe3a0_0007, 0xe1c1_00b0, 0xeaff_fffe],
    );
    let good = json!({"name":"gameplay", "rom":"input.gba", "step_limit":1000000,
        "completion":{"vblanks":3},
        "inputs":[{"vblank":0,"buttons":1},{"vblank":1,"buttons":17},{"vblank":2,"buttons":0}],
        "checks":[{"kind":"pixel","x":239,"y":159,"equals":255}]});
    let mut mismatch = good.clone();
    mismatch["name"] = json!("mismatch");
    mismatch["checks"][0]["equals"] = json!(0);
    let mut render_error = good.clone();
    render_error["name"] = json!("bad-video");
    render_error["rom"] = json!("bad-video.gba");
    let path = fixture.0.join("gameplay.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"version":2,"cases":[good,mismatch,render_error]})).unwrap(),
    )
    .unwrap();
    let first = run(&path);
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(first.stdout, run(&path).stdout);
    let report = report(&first);
    assert_eq!(report["format_version"], 2);
    assert_eq!(report["failed_count"], 2);
    let good = &report["cases"][0];
    assert_eq!(good["passed"], true);
    assert_eq!(good["state"]["vblanks"], 3);
    assert_eq!(good["state"]["captured_vblank"], 3);
    assert_eq!(good["inputs"][1]["buttons"], 17);
    assert_eq!(good["checks"][0]["actual"], 255);
    assert_eq!(report["cases"][1]["reason"], "assertion_failed");
    assert_eq!(report["cases"][2]["reason"], "render_error");
    assert_eq!(report["cases"][2]["state"]["vblanks"], 1);
    assert_eq!(report["cases"][2]["checks"], json!([]));
    assert!(report["cases"][2]["error"].is_string());
}

#[test]
fn suite_mode_rejects_other_modes_and_help_advertises_it() {
    for args in [
        vec!["--test-suite"],
        vec!["--test-suite", "missing.json", "--steps", "10"],
        vec!["--test-suite", "missing.json", "--window"],
        vec!["--test-suite", "missing.json", "--cpu-demo"],
        vec!["--test-suite", "missing.json", "--help"],
    ] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(stderr(&output).contains("usage:"));
    }
    let output = command().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("--test-suite PATH.json"));
}
