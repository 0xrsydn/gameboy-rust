# Cartridge files and execution

The desktop executable can load raw ROM bytes for terminal diagnostics or window execution.
This supports original test programs. It does not establish commercial-game compatibility.
For explicit pass/fail assertions instead of diagnostic limits, use the [headless ROM suite runner](../rom-tests.md).

```sh
direnv exec . cargo run --locked --release -- --rom path/to/original.gba --steps 100000
direnv exec . cargo run --locked --release -- --rom path/to/original.gba --window
```

Supply `--rom PATH` and exactly one execution mode: `--steps COUNT` or `--window`.
Option order does not matter. Terminal step counts accept decimal digits from 1 through 100,000,000.
Window mode optionally accepts `--frames COUNT`, from 1 through 100,000, for bounded presentation tests.
Reject zero, signed numbers, duplicate options, and combinations of terminal and window limits.
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
Ordinary cartridge bytes remain read-only.
Optional `--save-type flash64` or `--save-type flash128` enables [Flash identification and array reads](saves.md).
The desktop starts an erased device and does not load or write save files. Programming and erase remain diagnostic.
Optional `--rtc` selection enables the bounded [Game Pak GPIO and RTC calendar interface](cartridge-gpio.md).
It supports date/time reads, complete writes, reset, and explicit elapsed-time updates. RTC interrupts and persistence remain unsupported.
The desktop seeds UTC once, then supplies monotonic host elapsed seconds independently of GBA cycles and STOP.

Both modes call `bios::boot` with the loaded bytes.
With `--rtc`, the host then selects the cartridge peripheral before the first machine step.
Without that option, no RTC is attached. Duplicate `--rtc` options are rejected.
Save-device selection is independent of RTC selection. Omitting `--save-type` leaves save memory unmapped.
Unknown save types and duplicate options fail before file access. No ROM-based hardware detection is used.
The [original BIOS replacement](bios.md) executes its minimal boot before entering `0x08000000` in ARM System mode.
There is no command-line option for external BIOS files.
Unsupported services remain diagnostics. RegisterRamReset now accepts sound/serial reset flags, including `r0=0xff`, within the initialization subset.
Supported [sound](audio.md) and [serial](serial.md) behavior extends beyond reset, but remaining unsupported operations still produce diagnostics.
Accepting reset does not establish game compatibility.

## Terminal execution limits and results

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

Terminal mode has no window, keyboard input, or real-time pacing.
It does not enable frame capture or validate rendered output.

## Window execution

`--window` opens a native 240×160 window at 4× scale on the main thread.
The window presents [scanline-captured frames](display-timing.md#scanline-frame-capture) at VBlank entry.
Capture starts before BIOS boot. It has no demo-specific readiness checks and does not suppress startup images.
A black buffer appears before the first captured frame.
The host does not set display registers, initialize game RAM, or draw replacement game graphics.

Controls use the same focus correction and key mapping as the existing demos:

| Host key | GBA button |
| --- | --- |
| Arrows | D-pad |
| Z / X | A / B |
| Q / W | L / R |
| Enter | Start |
| Backspace | Select |

Escape or the window close control ends execution. Enter does not reset the emulator.
Focus loss releases all buttons but does not pause execution.
The host samples buttons before each execution slice, including while stopped.

Each slice executes at most 4,096 machine steps or stops at the next VBlank event.
The host processes window events between slices, not only when a frame completes.
A separate 400,000-step limit guards progress between VBlank events and persists across slices and STOP waits.
This limit permits a full frame of one-cycle instructions. It is not a wall-clock deadline.
HALT continues device clocks and frame publication.
Normal presentation targets approximately 59.73 frames per second, without frame skipping or accumulated catch-up work.
Slow hosts can run slower than real time. Host sleeping never advances emulated clocks.

In interactive mode, STOP retains the last image and changes the window title.
The host continues polling input and window-close events while GBA clocks remain frozen.
Only keypad conditions configured by the ROM through KEYCNT and IE can wake STOP.
A key press cannot wake a program that has not enabled a matching wake source.

`--frames COUNT` exits successfully after that many captured frames are submitted to the window.
It is a presentation limit, not proof that the ROM completed or passed a test.
STOP before that limit is an error instead of an indefinite wait for physical input.
Early window closure is also an error in this bounded mode; closure is normal in interactive mode.

CPU, DMA, video, and window errors end the session with a nonzero exit status.
The terminal report includes captured-frame count and the final machine state; stderr contains the error.
Rendering errors do not overwrite the last complete framebuffer.
Reports count captured frames, not idle redraws or window-event updates.

## Try an original ROM

Generate an original headerless input-test file, then open that file through the ROM loader:

```sh
direnv exec . cargo run --locked --example write_rom_demo -- /tmp/gba-input-demo.gba
direnv exec . cargo run --locked --release -- --rom /tmp/gba-input-demo.gba --window
```

The generator refuses to overwrite an existing file. Choose a new path if needed.
The CPU writes the palette: blue at rest, red while Z is held, and green while Right is held.
Right takes priority over Z. Other buttons have no visible effect in this small program.
No game or firmware content is included.

ROM windows can enable Darwin output with `--audio`; they are muted by default.
The option requires a usable default audio device. Device/configuration errors fail explicitly; omit the flag to run muted.
Focus loss and STOP clear queued audio. See [audio output and its limits](audio.md#darwin-output-adapter).
Terminal mode has no audio. Both modes lack save writing/persistence and per-step tracing.
Commercial-game compatibility remains unverified and unsupported.
Only load files that you may lawfully use. Do not commit game ROMs or firmware.
