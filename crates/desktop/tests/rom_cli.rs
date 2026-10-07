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

fn wave_setup() -> Vec<u32> {
    // Original ARM halfword stores and a status load; no external audio or assets.
    let mut code = Vec::new();
    for (address, value) in [
        (0x04000084, 0x80),
        (0x04000088, 0x200),
        (0x04000080, 0x4477),
        (0x04000082, 2),
        (0x04000090, 0xf012), // CPU bank 1 before selecting playback bank 1.
        (0x04000070, 0xc0),
        (0x04000072, 0x2000),
        (0x04000074, 0x87e0),
    ] {
        code.extend([0xe59f0000, 0xea000000, address]);
        code.extend([0xe59f1000, 0xea000000, value]);
        code.push(0xe1c010b0); // STRH r1,[r0]
    }
    code.extend([0xe59f0000, 0xea000000, 0x04000084, 0xe1d020b0, 0xeafffffe]);
    code
}

#[test]
fn audio_options_fail_before_file_access_unless_a_window_is_selected() {
    for tail in [
        vec!["--audio", "--steps", "1"],
        vec!["--audio", "--window", "--audio"],
    ] {
        let output = command()
            .args(["--rom", "missing-original.gba"])
            .args(tail)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(stderr(&output).contains("usage:"));
        assert!(!stderr(&output).contains("cannot load ROM"));
    }
    let output = command()
        .args(["--audio", "--window", "--rom", "missing-original.gba"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot load ROM"));
}

#[test]
fn original_wave_program_can_activate_channel3_without_host_audio() {
    let fixture = Fixture::new();
    let path = fixture.rom(&wave_setup());
    let original = fs::read(&path).unwrap();
    let output = run(&path, "1000");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("r2=0x00000084"),
        "{}",
        stdout(&output)
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

fn joybus_setup() -> Vec<u32> {
    // Original ARM: select Joybus, enable IRQ, supply a reply, inspect local status/data.
    vec![
        0xe59f1024, // LDR r1, RCNT
        0xe3a00903, // MOV r0, #0xc000
        0xe1c100b0, // STRH r0, [r1]
        0xe3a00040, // MOV r0, #0x40
        0xe1c100bc, // STRH r0, [r1, #12] (JOYCNT)
        0xe3a000a5, // MOV r0, #0xa5
        0xe5810020, // STR r0, [r1, #32] (JOY_TRANS)
        0xe1d122b4, // LDRH r2, [r1, #36] (JOYSTAT)
        0xe1d130bc, // LDRH r3, [r1, #12]
        0xe5914020, // LDR r4, [r1, #32]
        0xeafffffe, 0x04000134,
    ]
}

#[test]
fn original_joybus_program_reports_pending_data_without_a_remote_completion() {
    let fixture = Fixture::new();
    let path = fixture.rom(&joybus_setup());
    let original = fs::read(&path).unwrap();
    let output = run(&path, "1000");
    assert!(output.status.success(), "{}", stderr(&output));
    for expected in [
        "r2=0x00000002",
        "r3=0x00000040",
        "r4=0x000000a5",
        "IRQ entries: 0",
    ] {
        assert!(stdout(&output).contains(expected), "{}", stdout(&output));
    }
    assert_eq!(fs::read(path).unwrap(), original);
}

fn rtc_setup() -> Vec<u32> {
    // Original LDR/MOV/STRH/LDRH/loop: enable GPIO reads, then inspect control.
    vec![
        0xe59f100c, 0xe3a00001, 0xe1c100b0, 0xe1d120b0, 0xeafffffe, 0x080000c8,
    ]
}

fn rtc_calendar_setup() -> Vec<u32> {
    // Original unrolled ARM GPIO driver. Write 2024-02-29 12:34:56, then read the hour.
    // Keep executable instructions past the GPIO overlay at ROM offsets c4..c9.
    let mut code = vec![0xe1a00000; 64];
    code[0] = 0xea00003e; // B 0x08000100
    code.extend([0xe59f1000, 0xea000000, 0x080000c4]); // LDR r1; skip literal.
    fn store(code: &mut Vec<u32>, value: u32, offset: u32) {
        code.extend([0xe3a00000 | value, 0xe1c100b0 | offset]);
    }
    fn send(code: &mut Vec<u32>, byte: u8, command: bool) {
        for bit in 0..8 {
            let shift = if command { 7 - bit } else { bit };
            let pins = 4 | (u32::from((byte >> shift) & 1) << 1);
            store(code, pins, 0);
            store(code, pins | 1, 0);
        }
    }
    store(&mut code, 1, 4); // Read enable.
    store(&mut code, 1, 0);
    store(&mut code, 7, 2);
    store(&mut code, 5, 0);
    send(&mut code, 0x64, true);
    for byte in [0x24, 2, 0x29, 4, 0x12, 0x34, 0x56] {
        send(&mut code, byte, false);
    }
    store(&mut code, 1, 0); // End write and begin time read.
    store(&mut code, 5, 0);
    send(&mut code, 0x67, true);
    store(&mut code, 5, 2);
    code.push(0xe3a02000); // MOV r2,#0
    for bit in 0..8 {
        store(&mut code, 4, 0);
        store(&mut code, 5, 0);
        code.extend([0xe1d130b0, 0xe1a030a3, 0xe2033001, 0xe1822003 | (bit << 7)]);
    }
    store(&mut code, 1, 0); // Aborting the remaining read bytes preserves calendar state.
    code.push(0xeafffffe);
    code
}

#[test]
fn rtc_calendar_commands_run_through_original_arm_code_with_host_clock_enabled() {
    let fixture = Fixture::new();
    let path = fixture.rom(&rtc_calendar_setup());
    let output = command()
        .arg("--rom")
        .arg(&path)
        .args(["--rtc", "--steps", "5000"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("r2=0x00000092"),
        "{}",
        stdout(&output)
    );
}

fn flash_setup() -> Vec<u32> {
    // Original ARM unlock/identify, then install a byte reader in IWRAM and execute it.
    let mut code = Vec::new();
    let mut literals = Vec::new();
    fn load(code: &mut Vec<u32>, literals: &mut Vec<(usize, u32, u32)>, register: u32, value: u32) {
        literals.push((code.len(), register, value));
        code.push(0);
    }
    load(&mut code, &mut literals, 1, 0x0e005555);
    code.extend([0xe3a000aa, 0xe5c10000]);
    load(&mut code, &mut literals, 2, 0x0e002aaa);
    code.extend([0xe3a00055, 0xe5c20000, 0xe3a00090, 0xe5c10000]);
    load(&mut code, &mut literals, 3, 0x0e000000);
    load(&mut code, &mut literals, 4, 0x03000000);
    for (offset, instruction) in [(0, 0xe5d35000), (4, 0xe5d36001), (8, 0xeafffffe)] {
        load(&mut code, &mut literals, 0, instruction);
        code.push(0xe5840000 | offset);
    }
    code.push(0xe12fff14); // BX r4
    for (at, register, value) in literals {
        let offset = ((code.len() - at) * 4 - 8) as u32;
        code[at] = 0xe59f0000 | (register << 12) | offset;
        code.push(value);
    }
    code
}

#[test]
fn explicit_flash_selection_runs_ram_byte_reader_and_leaves_files_unchanged() {
    let fixture = Fixture::new();
    let path = fixture.rom(&flash_setup());
    let bytes = fs::read(&path).unwrap();
    assert!(stderr(&run(&path, "1000")).contains("unmapped memory at 0x0e005555"));
    for (device, id) in [("flash64", "r6=0x0000001c"), ("flash128", "r6=0x00000009")] {
        let output = command()
            .args(["--save-type", device, "--rtc", "--rom"])
            .arg(&path)
            .args(["--steps", "1000"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let text = stdout(&output);
        assert!(text.contains("identification/bank reads only"));
        assert!(text.contains("r5=0x000000c2"), "{text}");
        assert!(text.contains(id), "{text}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1); // No save file is invented.
}

#[test]
fn invalid_save_type_options_fail_before_file_access() {
    for extra in [
        vec!["--save-type"],
        vec!["--save-type", "sram"],
        vec!["--save-type", ""],
        vec!["--save-type", "flash128", "--save-type", "flash64"],
    ] {
        let output = command()
            .args(["--rom", "missing.gba", "--steps", "1"])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("usage:"));
        assert!(!stderr(&output).contains("cannot load"));
    }
}

#[test]
fn rtc_selection_is_explicit_and_does_not_modify_the_rom_file() {
    let fixture = Fixture::new();
    let path = fixture.rom(&rtc_setup());
    let original = fs::read(&path).unwrap();
    let absent = run(&path, "100");
    assert_eq!(absent.status.code(), Some(1));
    assert!(stderr(&absent).contains("read-only memory at 0x080000c8"));
    let selected = command()
        .arg("--rtc")
        .arg("--rom")
        .arg(&path)
        .args(["--steps", "100"])
        .output()
        .unwrap();
    assert!(selected.status.success(), "{}", stderr(&selected));
    assert!(stdout(&selected).contains("RTC selected: UTC at startup"));
    assert!(stdout(&selected).contains("r2=0x00000001"));
    assert_eq!(fs::read(&path).unwrap(), original);
    let duplicate = command()
        .arg("--rom")
        .arg(&path)
        .args(["--rtc", "--rtc", "--steps", "100"])
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(stderr(&duplicate).contains("usage:"));
    assert!(duplicate.stdout.is_empty());
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
    assert!(stdout(&output).contains("ROM windows offer --audio on macOS. No saves."));
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

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires a focused native window and audio device; plays an original wave tone"]
fn native_rom_audio_window_routes_core_samples_to_output() {
    let fixture = Fixture::new();
    let path = fixture.rom(&wave_setup());
    let mut child = command()
        .arg("--rom")
        .arg(&path)
        .args(["--window", "--audio", "--frames", "120"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "audio window timed out: {} {}",
                stdout(&output),
                stderr(&output)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        stdout(&output),
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("Audio enabled: CoreAudio:"), "{text}");
    assert!(text.contains("Captured ROM frames: 120"), "{text}");
    let nonzero: u64 = text
        .split("nonzero output frames: ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(nonzero > 0, "{text}");
    assert!(text.contains("dropped input frames: 0"), "{text}");
}

#[test]
#[ignore = "requires a logged-in native desktop session"]
fn native_rom_window_presents_file_bytes_and_reports_stop_cpu_and_video_errors() {
    use std::{
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    fn window(path: &Path, rtc: bool, save: Option<&str>) -> Output {
        let mut command = command();
        if rtc {
            command.arg("--rtc");
        }
        if let Some(device) = save {
            command.args(["--save-type", device]);
        }
        let mut child = command
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
    let output = window(&path, false, None);
    assert!(
        output.status.success(),
        "{} {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("Captured ROM frames: 3"));
    assert!(stdout(&output).contains("Result: window frame limit reached"));
    let wave_path = fixture.rom(&wave_setup());
    let wave_output = window(&wave_path, false, None);
    assert!(wave_output.status.success(), "{}", stderr(&wave_output));
    assert!(stdout(&wave_output).contains("Captured ROM frames: 3"));
    assert!(stdout(&wave_output).contains("r2=0x00000084"));
    let joybus_path = fixture.rom(&joybus_setup());
    let joybus_output = window(&joybus_path, false, None);
    assert!(joybus_output.status.success(), "{}", stderr(&joybus_output));
    for expected in [
        "Captured ROM frames: 3",
        "r2=0x00000002",
        "r3=0x00000040",
        "r4=0x000000a5",
        "IRQ entries: 0",
    ] {
        assert!(
            stdout(&joybus_output).contains(expected),
            "{}",
            stdout(&joybus_output)
        );
    }
    let rtc_path = fixture.rom(&rtc_setup());
    let rtc_output = window(&rtc_path, true, None);
    assert!(rtc_output.status.success(), "{}", stderr(&rtc_output));
    assert!(stdout(&rtc_output).contains("Captured ROM frames: 3"));
    assert!(stdout(&rtc_output).contains("r2=0x00000001"));
    let calendar_path = fixture.rom(&rtc_calendar_setup());
    let calendar_output = window(&calendar_path, true, None);
    assert!(
        calendar_output.status.success(),
        "{}",
        stderr(&calendar_output)
    );
    assert!(stdout(&calendar_output).contains("Captured ROM frames: 3"));
    assert!(stdout(&calendar_output).contains("r2=0x00000092"));
    let flash_path = fixture.rom(&flash_setup());
    let flash_output = window(&flash_path, true, Some("flash128"));
    assert!(flash_output.status.success(), "{}", stderr(&flash_output));
    assert!(stdout(&flash_output).contains("Captured ROM frames: 3"));
    assert!(stdout(&flash_output).contains("r5=0x000000c2"));
    assert!(stdout(&flash_output).contains("r6=0x00000009"));
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
        let output = window(&path, false, None);
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
