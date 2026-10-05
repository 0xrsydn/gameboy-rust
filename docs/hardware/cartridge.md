# Cartridge files and terminal execution

The desktop executable can load raw ROM bytes and run a bounded diagnostic session.
This supports original test programs. It does not establish commercial-game compatibility.

```sh
direnv exec . cargo run --locked --release -- --rom path/to/original.gba --steps 100000
```

Both options are required. Their order does not matter.
`COUNT` must contain decimal digits and be between 1 and 100,000,000 inclusive.
Do not combine ROM mode with demo or help options. Bare file paths are not accepted.
Quote paths that contain spaces. File extensions do not affect loading.

## Loading and boot

`crates/desktop/src/cartridge.rs` owns file access, argument checks, execution limits, and reports.
The core remains independent of the filesystem and window library.
The loader opens a regular file read-only and accepts 4 bytes through 32 MiB inclusive.
It checks metadata before and after opening, then independently bounds the read to the limit plus one byte.
This bounds file content reads even if the file grows. It is not a filesystem race protection mechanism.
Directories and other non-regular files are rejected. Symlinks to regular files are accepted.
Paths retain their operating-system representation and appear quoted in diagnostics.
The host filesystem can reject filenames, including invalid UTF-8 on Darwin.

Bytes are not padded, patched, or written back.
The loader does not validate a cartridge header, logo, or checksum.
It does not decode archives or ELF executables. Supply a raw binary with an ARM instruction at offset zero.
Headerless original programs are accepted.
Reads past supplied cartridge bytes remain explicit memory errors, not open-bus values.
All three Game Pak ROM windows use the same supplied bytes and existing wait-state rules.
Cartridge writes, save devices, and cartridge peripherals remain unsupported.

The runner calls `bios::boot` with the loaded bytes.
The [original BIOS replacement](bios.md) executes its minimal boot before entering `0x08000000` in ARM System mode.
There is no command-line option for external BIOS files.
Unsupported services remain diagnostics. In particular, RegisterRamReset sound/serial flags, including `r0=0xff`, still fail.

## Execution limits and results

The budget counts successful machine steps, including BIOS boot instructions.
Each CPU instruction, IRQ entry, DMA unit, or HALT idle batch consumes one step.
The budget is not an instruction-only, cycle, frame, or wall-clock limit.
HALT continues advancing devices within the budget.
STOP returns immediately because terminal mode has no input source to wake the machine.
STOP on the final allowed step is reported as STOP, not as budget exhaustion.
A failed CPU or DMA step consumes no budget and retains the machine state from earlier successful steps.

The report includes:

- Quoted path, byte count, and requested step limit.
- Stop reason and successful-step counters by kind.
- Nominal emulated cycles, PC, CPSR, CPU mode, and ARM/Thumb state.
- HALT/STOP flags and all active registers r0–r15.

Exit status zero means the step limit was reached or STOP was entered.
It does not mean the program completed or passed a hardware test.
Invalid arguments, loading failures, output failures, and emulation diagnostics produce a nonzero exit status.
For emulation diagnostics, stdout contains the final machine state and stderr contains the core error.

ROM mode has no window, keyboard input, audio, saves, per-step trace, or real-time pacing.
It does not enable frame capture or validate rendered output.
Use the existing demos for window and input tests.
Only load files that you may lawfully use. Do not commit game ROMs or firmware.
