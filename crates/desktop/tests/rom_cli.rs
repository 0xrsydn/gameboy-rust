//! Process-level checks use only original instructions and temporary raw files.
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use gba_core::memory::ROM_CAPACITY;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gameboy-rust-rom-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn rom(&self, code: &[u32]) -> PathBuf {
        let path = self.0.join("original program.gba");
        let bytes: Vec<u8> = code.iter().flat_map(|word| word.to_le_bytes()).collect();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gameboy-rust"))
}
fn run(path: &Path, steps: &str) -> Output {
    command()
        .arg("--rom")
        .arg(path)
        .args(["--steps", steps])
        .output()
        .unwrap()
}
fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn raw_original_program_runs_with_a_bounded_report_without_changing_the_file() {
    let fixture = Fixture::new();
    let path = fixture.rom(&[0xe3a0_002a, 0xeaff_fffe]);
    let original = fs::read(&path).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let output = run(&path, "100");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for expected in [
        "(8 bytes)",
        "Original BIOS replacement",
        "Result: step limit reached",
        "Steps: 100",
        "PC=0x08000004",
        "r0=0x0000002a",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn stop_is_a_normal_bounded_outcome() {
    let fixture = Fixture::new();
    let path = fixture.rom(&[0xef03_0000, 0xeaff_fffe]);
    let output = run(&path, "1000");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Result: STOP; input required"));
    assert!(stdout(&output).contains("stopped=true"));
    assert!(!stdout(&output).contains("Steps: 1000;"));
}

#[test]
fn unsupported_instruction_and_fallthrough_fail_with_machine_state() {
    let fixture = Fixture::new();
    for code in [[0xee00_0000], [0xe3a0_002a]] {
        let path = fixture.rom(&code);
        let output = run(&path, "100");
        assert_eq!(output.status.code(), Some(1));
        assert!(stdout(&output).contains("Result: emulation diagnostic"));
        assert!(stdout(&output).contains("r15=0x080000"));
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn missing_short_directory_and_oversized_files_fail_before_execution() {
    let fixture = Fixture::new();
    let path = fixture.0.join("input.gba");
    for candidate in [&path, &fixture.0] {
        let output = run(candidate, "1");
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("cannot load ROM"));
        assert!(output.stdout.is_empty());
    }
    for size in [0, 1, 2, 3, ROM_CAPACITY + 1] {
        File::create(&path).unwrap().set_len(size as u64).unwrap();
        let output = run(&path, "1");
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains(if size < 4 {
            "at least 4 bytes"
        } else {
            "32 MiB"
        }));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn exact_capacity_is_accepted_and_odd_lengths_are_not_padded() {
    let fixture = Fixture::new();
    let path = fixture.rom(&[0xeaff_fffe]);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(ROM_CAPACITY as u64)
        .unwrap();
    let output = run(&path, "100");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("(33554432 bytes)"));
    fs::write(&path, [0xfe, 0xff, 0xff, 0xea, 0x42]).unwrap();
    let output = run(&path, "100");
    assert!(output.status.success());
    assert!(stdout(&output).contains("(5 bytes)"));
}

#[test]
fn invalid_options_fail_before_opening_a_file() {
    for count in ["0", "-1", "100000001", "18446744073709551616"] {
        let output = run(Path::new("does-not-exist.gba"), count);
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("usage:"));
        assert!(!stderr(&output).contains("cannot load"));
    }
    for extra in ["--cpu-demo", "--help", "--rom", "--steps"] {
        let output = command()
            .args(["--rom", "missing.gba", "--steps", "1", extra])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("usage:"));
    }
    let output = command().args(["--steps", "1"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn help_describes_terminal_rom_mode_and_existing_cpu_demo_still_runs() {
    let output = command().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(stdout(&output).contains("--rom PATH --steps COUNT"));
    assert!(stdout(&output).contains("--rom PATH --window"));
    assert!(stdout(&output).contains("ROM modes have no audio or saves"));
    let output = command().arg("--cpu-demo").output().unwrap();
    assert!(output.status.success());
    assert!(stdout(&output).contains("Exception demo"));
}

#[test]
fn window_options_and_bad_files_fail_without_opening_a_window() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("missing.gba");
    for extra in [
        vec!["--steps", "1"],
        vec!["--frames", "0"],
        vec!["--frames", "100001"],
        vec!["--window"],
        vec!["--help"],
    ] {
        let output = command()
            .arg("--rom")
            .arg(&missing)
            .arg("--window")
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("usage:"));
        assert!(output.stdout.is_empty());
    }
    let output = command()
        .arg("--rom")
        .arg(&missing)
        .arg("--window")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cannot load ROM"));
    assert!(output.stdout.is_empty());
}

#[test]
#[ignore = "requires a logged-in native desktop session"]
fn native_rom_window_presents_file_bytes_and_reports_stop_cpu_and_video_errors() {
    use std::{
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    fn window(path: &Path) -> Output {
        let mut child = command()
            .arg("--rom")
            .arg(path)
            .args(["--window", "--frames", "3"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(20) {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "ROM window did not exit: {} {}",
                    stdout(&output),
                    stderr(&output)
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
    let fixture = Fixture::new();
    let path = fixture.0.join("original-input.gba");
    fs::write(&path, gba_demos::input_rom()).unwrap();
    let output = window(&path);
    assert!(
        output.status.success(),
        "{} {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("Captured ROM frames: 3"));
    assert!(stdout(&output).contains("Result: window frame limit reached"));
    for (code, error) in [
        (vec![0xef03_0000, 0xeaff_fffe], "entered STOP"),
        (vec![0xee00_0000], "instruction"),
        (
            vec![0xe3a0_0006, 0xe3a0_1301, 0xe1c1_00b0, 0xeaff_fffe],
            "6",
        ),
    ] {
        fs::write(
            &path,
            code.iter()
                .flat_map(|word: &u32| word.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let output = window(&path);
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        assert!(stdout(&output).contains("Result: ROM window diagnostic"));
        assert!(stdout(&output).contains("Captured ROM frames: 0"));
        assert!(stdout(&output).contains("PC="));
        assert!(stderr(&output).contains(error), "{}", stderr(&output));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_reach_the_loader_without_panicking() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let fixture = Fixture::new();
    // Darwin filesystems can reject invalid UTF-8. Do not require creating one.
    let path = fixture.0.join(OsStr::from_bytes(b"missing-\xff.gba"));
    let output = run(&path, "1");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cannot load ROM"));
    assert!(!stderr(&output).contains("usage:"));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn unicode_paths_and_regular_file_symlinks_work() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let original = fixture.rom(&[0xeaff_fffe]);
    for name in ["original-λ.gba", "original-link.gba"] {
        let path = fixture.0.join(name);
        symlink(&original, &path).unwrap();
        let output = command()
            .args(["--steps", "100", "--rom"])
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stdout(&output).contains("Result: step limit reached"));
    }
}
