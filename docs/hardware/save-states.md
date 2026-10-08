# Save states

A save state freezes the emulated machine at a step boundary and restores it later.
It is separate from the game's own save (Flash) and does not replace it: [Flash persistence](saves.md) still needs the game's SAVE command and a clean exit.

## Desktop use

Window mode binds two keys while the window has focus:

| Key | Action |
| --- | --- |
| F5 | Write the state file |
| F9 | Load the state file |

The slot is `--state-file PATH`. Without it, the ROM path with its extension replaced by `.state` is used (`roms/pokemon-emerald.gba` becomes `roms/pokemon-emerald.state`).
Only one slot exists. F5 replaces it, via a unique sibling temporary file that is synced and renamed, so an interrupted write keeps the previous state.
State files are created with mode `0600`. The option is accepted only with `--window`.

A rejected load (missing file, wrong ROM, corrupt or older-format file) is reported in the terminal and the game keeps running unchanged.
Loading discards queued host audio and shows the state's last captured frame immediately.
The Flash image inside the state replaces the running one. It is written to `--save-file` only on a clean exit, and only if it differs from the last persisted bytes.
The RTC resumes at the saved calendar value and then follows host elapsed time again.
Quitting with Escape does not write a state. Press F5 first.

## What a state holds

`Machine::save_state` and `Machine::load_state` (`crates/gba-core/src/machine/snapshot.rs`) encode:

- CPU registers, banks, status, instruction buffer, and retained fetch history.
- All RAMs, VRAM, palette, and OAM, plus the Flash image and its modified flag.
- Every device: timers and the shared prescaler, DMA channels and latches, display position, interrupts, keypad, serial, audio (FIFOs, PSG channels, wave RAM), Flash and RTC/GPIO state machines.
- Video latches: affine accumulators, mosaic and window counters, sprite row pipeline, and the double-buffered scanline capture.
- Game Pak prefetch, IWRAM bus lanes, BIOS protected-read snapshot, the device cycle count, and the last step timing.

It does not hold the ROM or firmware bytes, the host's choice of cartridge hardware and save device, row-capture and audio-capture enablement, or host audio queues and drop counters.
An enabled audio capture keeps running and resumes its sample phase.

## Format and checks

The layout is `GBAS`, a format version, ROM length and fingerprint, firmware presence and fingerprint, cartridge setup, a length-prefixed payload, and a trailing checksum.
Fingerprints and the checksum are 64-bit FNV-1a: they catch mismatched images and accidental corruption, not tampering.
The payload is positional with a four-byte tag per section, so a reader out of step fails at the next boundary.
Integers are little-endian. Allocations are bounded by fixed memory sizes; no length field drives an unbounded allocation.

`load_state` verifies the magic, version, checksum, and ROM/firmware/cartridge identity before changing anything.
These failures leave the machine untouched.
A failure after that point returns `RestoreFailed`; it indicates an encoder defect, and the machine must be rebuilt.
Any format change bumps the version. Older states are rejected, not migrated.

## Limits

- `save_state` fails with `MidStep` inside a step and `Unsupported` while the machine holds a deferred diagnostic error (a stored failed instruction fetch or failed captured frame). Saving at a later step boundary normally works.
- States are for the same build line: any change to encoded hardware state needs a version bump and invalidates old files.
- Checksum and fingerprints are not authentication. Do not load untrusted state files expecting more than memory safety.
- A state restores hardware, not the host: window frame counters, audio device buffers, and the host RTC baseline are not part of it.

## Evidence

`crates/gba-core/tests/save_state.rs` runs original programs, snapshots mid-run, restores into a freshly built machine, and compares per-frame hashes of pixels, captured audio, registers, and the device clock, then compares the final state bytes.
It also restores a Flash program operation in flight and checks that every rejected input leaves the machine unchanged.
Each encoded type destructures its struct exhaustively, so a new field is a compile error until it is encoded or excluded with a reason.
Per-type unit tests check round trips, rejection of every truncated prefix, and invalid discriminants.
