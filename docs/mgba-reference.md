# Differential testing against mGBA

`gba-diff` runs a ROM in this emulator and in [mGBA](https://mgba.io) with the same inputs, then reports where they disagree.
mGBA is a widely used, accuracy-focused GBA emulator. It is a reference, not ground truth: when the two disagree,
hardware-verified results such as the [mGBA test suite](public-mgba-suite.md) decide which side to change.

Nothing from mGBA is committed here. A script builds mGBA's core library locally from a pinned release tarball and links it
into `mgba-ref`, a small original C program (`tools/mgba-ref/mgba_ref.c`) that `gba-diff` drives over stdin/stdout.
`gba-core` never links mGBA, and `gba-diff` only spawns the `mgba-ref` process.
mGBA source is useful for understanding behavior, but this project's architecture differs; adapt ideas rather than copying files.
mGBA is MPL-2.0, so any copied file would carry that license.

## Build the reference

Requires cmake and a C compiler. cmake is not in the dev shell; take it from the pinned nixpkgs:

```sh
direnv exec . nix shell --inputs-from . nixpkgs#cmake -c \
  python3 tools/prepare_mgba_reference.py roms/mgba-reference
```

The destination must be new, and outside the repository or inside the ignored `roms/`.
The script downloads mGBA 0.10.5 (`tools/mgba-reference.lock.json` pins its size and SHA-256), extracts it with path checks,
and builds a static library without frontends, OpenGL, scripting, debuggers, or optional libraries.
It writes `mgba-ref`, plus `mgba-perf` for later benchmarking. Use `--tarball PATH` for an offline copy of the same tarball.
Build logs stay in the destination. Verified on Darwin arm64 with Nix cmake 4.4.

## Frame comparison

```sh
direnv exec . cargo run --locked --release -p gba-diff -- \
  --reference roms/mgba-reference/mgba-ref --rom roms/game.gba --save-type flash128 \
  --frames 600 --input 120:start:4,200:a:4 --out /tmp/gba-diff
```

After every frame (each VBlank), it compares EWRAM, IWRAM, palette RAM, VRAM, OAM, 30 readable I/O registers, and all
pixels at 5 bits per channel. It prints the first divergent frame in detail, then one summary line per later divergent
frame (`--max-report`). `--out` writes this emulator's frame, mGBA's frame, and a red difference mask as PPM files.
`--ignore START-END[,...]` skips address ranges. Inputs use `FRAME:BUTTON:HOLD`, counted from frame 0.
Exit status is 0 without divergence, 1 with one, and 2 for usage or setup errors.

mGBA chooses its own save type from its game database or on first access; `--save-type` applies only to this emulator.

## Instruction lockstep

```sh
direnv exec . cargo run --locked --release -p gba-diff -- \
  --reference roms/mgba-reference/mgba-ref --rom roms/game.gba --trace 2000000
```

Both sides report every instruction executed outside the BIOS (`0x0000–0x3fff`), with the registers and cycle count
right after it. BIOS internals are skipped because the two BIOS implementations legitimately differ: mGBA has high-level
BIOS calls, while this emulator runs its original replacement firmware. An instruction that enters the BIOS, such as a
`SWI`, is reported once control leaves the BIOS again, so its registers are compared after the call. Its cycle cost
compares two different BIOS implementations; expect drift there.
mGBA's idle-loop skipping is disabled so that its cycle count stays exact.

- **State divergence** stops the run: a different instruction address, a different register, next PC, or CPSR.
  The last few instructions on both sides are printed. mGBA's pipelined r15 is normalized to the next instruction address.
- **Cycle drift** is how far this emulator's clock runs ahead (+) or behind (−) mGBA's since the cartridge entry.
  Costs often differ only in attribution, such as a store charged on one side and the following fetch on the other; such
  pairs cancel and are not reported. A drift is reported when the offset settles at a new value for four instructions, or
  has moved 64 cycles since the last report. Each report lists the instructions whose costs differed.

Drift eventually causes state divergence: one side takes an interrupt a few instructions earlier, for example.
Fix the first drift before reading later divergences.

## Start alignment

Both emulators approximate the real BIOS exit, and neither start is hardware-verified.
By default `gba-diff` moves mGBA to this emulator's start: the same scanline and line cycle, and the same `DISPCNT`.
It prints both original values first. `--no-align` keeps mGBA's own start (scanline 126 with forced blank).

The boot replacement already matches mGBA's SOUNDBIAS (`0x0200`) and its cleared r0–r12 and flags; see [BIOS](hardware/bios.md).

## Findings so far

Measured 2026-11-13 on Darwin arm64 with the [pinned suite ROM](public-mgba-suite.md), before any timing changes:

- After start alignment, the first six frames match exactly: all RAM regions, compared I/O, and every pixel.
  Before alignment, every frame differed (start scanline 0 versus 126, `DISPCNT`, `SOUNDBIAS`, and leftover boot registers).
- Lockstep matches 249,014 instructions before the first state divergence, when mGBA takes a VBlank IRQ one instruction earlier.
- Thumb code running from ROM, such as the startup copy loop at `0x080001a6` (`LDMIA r1!,{r0}` and `SUB r3,#4`), costs this
  emulator 2 more cycles per instruction than mGBA. This is not yet checked against hardware.
- The suite's timing calibration also read exactly 2 cycles high in every case, including IWRAM code. That was a separate
  timer start delay, now [modeled](hardware/timers-irq.md); it did not change the copy-loop drift above.
- Thumb word stores to SRAM cost 8–10 cycles here and 26 in mGBA. The suite has no SRAM timing case, so this stays open
  until hardware evidence decides it.
- mGBA begins HBlank 1008 cycles into a line; this emulator uses 1006.

With a local Pokémon FireRed dump (not distributed), lockstep matches 1,021 instructions:

- The game's `RegisterRamReset` call (`SWI 0x01`) costs about 948,000 more cycles in this emulator's BIOS than in mGBA's
  high-level implementation. Real firmware cost lies somewhere between; it is not measured here.
- The game starts an immediate DMA3 transfer and reads `DMA3CNT` back in the next instruction. mGBA still reports the
  enable bit (`0x84000000`) because its transfer has not started yet; this emulator has already finished it (`0x04000000`).
  GBATEK documents a short DMA start delay; the game's following branch differs.

## Limits

- Only the GBA is supported. mGBA's Game Boy core is built but not driven.
- No BIOS image is passed to mGBA. A real BIOS would make BIOS-internal comparison possible, but none is distributed here.
- Paths passed to `mgba-ref` must be UTF-8 without whitespace.
- Audio is not compared.
