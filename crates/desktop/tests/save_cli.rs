//! Save/restart process tests use original ARM instructions and temporary raw images.
#![cfg(unix)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "gba-save-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn rom(&self, name: &str, code: &[u32]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(
            &path,
            code.iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        path
    }
    fn save(&self) -> PathBuf {
        self.0.join("original.sav")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command(rom: &Path, save: &Path, device: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gameboy-rust"));
    c.arg("--rom")
        .arg(rom)
        .args(["--save-type", device, "--save-file"])
        .arg(save);
    c
}
fn text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn load(code: &mut Vec<u32>, register: u32, value: u32) {
    code.extend([0xe59f0000 | register << 12, 0xea000000, value]);
}
fn writer(value: u8, fail: bool) -> Vec<u32> {
    let mut code = Vec::new();
    for (address, byte) in [
        (0x0e005555, 0xaa),
        (0x0e002aaa, 0x55),
        (0x0e005555, 0xa0),
        (0x0e000000, value),
    ] {
        load(&mut code, 0, address);
        code.extend([0xe3a01000 | u32::from(byte), 0xe5c01000]);
    }
    code.extend([0xe1a00000; 700]); // Allow pending program to complete before failure or exit.
    code.push(if fail { 0xee000000 } else { 0xeafffffe });
    code
}
fn reader() -> Vec<u32> {
    let mut code = Vec::new();
    load(&mut code, 3, 0x0e000000);
    load(&mut code, 4, 0x03000000);
    for (offset, instruction) in [(0, 0xe5d35000), (4, 0xeafffffe)] {
        load(&mut code, 0, instruction);
        code.push(0xe5840000 | offset);
    }
    code.push(0xe12fff14);
    code
}
fn run(rom: &Path, save: &Path, device: &str) -> Output {
    command(rom, save, device)
        .args(["--steps", "2000"])
        .output()
        .unwrap()
}

#[test]
fn separate_processes_program_reload_and_retain_previous_images() {
    for (device, size) in [("flash64", 65536), ("flash128", 131072)] {
        let f = Fixture::new();
        let write = f.rom("writer.gba", &writer(0xa5, false));
        let bytes = fs::read(&write).unwrap();
        let mut permissions = fs::metadata(&write).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&write, permissions).unwrap();
        let save = f.save();
        let output = run(&write, &save, device);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains("Save persisted on clean exit"));
        let original = fs::read(&save).unwrap();
        assert_eq!(original.len(), size);
        assert_eq!(original[0], 0xa5);
        assert!(original[1..].iter().all(|&v| v == 255));
        let read = f.rom("reader.gba", &reader());
        let output = run(&read, &save, device);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains("r5=0x000000a5"));
        assert!(text(&output).contains("Save unchanged"));
        let write2 = f.rom("writer2.gba", &writer(0xa0, false));
        let output = run(&write2, &save, device);
        assert!(output.status.success(), "{}", text(&output));
        assert!(text(&output).contains("Previous save backup"));
        let backups: Vec<_> = fs::read_dir(&f.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original);
        assert_eq!(fs::read(&save).unwrap()[0], 0xa0);
        assert_eq!(fs::read(&write).unwrap(), bytes);
        assert!(!f.0.join("original.sav.lock").exists());
    }
}

#[test]
fn diagnostic_after_completed_program_never_persists_or_creates_backups() {
    for existing in [false, true] {
        let f = Fixture::new();
        let rom = f.rom("failure.gba", &writer(0x12, true));
        let save = f.save();
        let original = vec![255; 131072];
        if existing {
            fs::write(&save, &original).unwrap();
        }
        let output = run(&rom, &save, "flash128");
        assert!(!output.status.success());
        assert!(text(&output).contains("unsupported instruction"));
        if existing {
            assert_eq!(fs::read(&save).unwrap(), original);
        } else {
            assert!(!save.exists());
        }
        assert_eq!(
            fs::read_dir(&f.0).unwrap().count(),
            if existing { 2 } else { 1 }
        );
    }
}

#[test]
fn clean_terminal_stop_with_an_incomplete_command_refuses_persistence() {
    let f = Fixture::new();
    let mut code = Vec::new();
    load(&mut code, 0, 0x0e005555);
    code.extend([0xe3a010aa, 0xe5c01000, 0xef030000, 0xeafffffe]);
    let rom = f.rom("incomplete.gba", &code);
    let output = run(&rom, &f.save(), "flash128");
    assert!(!output.status.success());
    assert!(
        text(&output).contains("Flash command/operation is incomplete"),
        "{}",
        text(&output)
    );
    assert!(!f.save().exists());
}

#[test]
fn existing_lock_blocks_execution_without_changing_files() {
    let f = Fixture::new();
    let rom = f.rom("writer.gba", &writer(0xa5, false));
    let lock = f.0.join("original.sav.lock");
    fs::write(&lock, b"other process").unwrap();
    let output = run(&rom, &f.save(), "flash128");
    assert!(!output.status.success());
    assert!(text(&output).contains("cannot lock save file"));
    assert_eq!(fs::read(lock).unwrap(), b"other process");
    assert!(!f.save().exists());
}

#[test]
#[ignore = "requires a logged-in desktop session"]
fn native_rom_window_save_reloads_in_another_process() {
    let f = Fixture::new();
    let write = f.rom("writer.gba", &writer(0xa5, false));
    let output = command(&write, &f.save(), "flash128")
        .args(["--window", "--frames", "3"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Save persisted on clean exit"));
    let read = f.rom("reader.gba", &reader());
    let output = run(&read, &f.save(), "flash128");
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("r5=0x000000a5"));
    let baseline = fs::read(f.save()).unwrap();
    let fail = f.rom("failure.gba", &writer(0xa0, true));
    let output = command(&fail, &f.save(), "flash128")
        .args(["--window", "--frames", "3"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("unsupported instruction"));
    assert_eq!(fs::read(f.save()).unwrap(), baseline);
    let mut code = Vec::new();
    load(&mut code, 0, 0x0e005555);
    code.extend([0xe3a010aa, 0xe5c01000, 0xeafffffe]);
    let incomplete = f.rom("incomplete.gba", &code);
    let output = command(&incomplete, &f.save(), "flash128")
        .args(["--window", "--frames", "3"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("Flash command/operation is incomplete"));
    assert_eq!(fs::read(f.save()).unwrap(), baseline);
    assert!(!fs::read_dir(&f.0).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".bak.")));
}
