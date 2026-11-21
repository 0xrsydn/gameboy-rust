# Differential testing against mGBA

`gba-diff` runs a ROM in this emulator and in [mGBA](https://mgba.io) with the same inputs, then reports where they disagree.
mGBA is a widely used, accuracy-focused GBA emulator. It is a reference, not ground truth: when the two disagree,
hardware-verified results such as the [mGBA test suite](public-mgba-suite.md) decide which side to change.
[Mesen 2](mesen-reference.md) is a second reference that `gba-diff` drives the same way; it runs this emulator's
firmware, so neither the start alignment nor the BIOS-cost findings below apply to it.

Nothing from mGBA is committed here. A script builds mGBA's core library locally from a pinned release tarball and links it
into `mgba-ref`, a small original C program (`tools/mgba-ref/mgba_ref.c`) that `gba-diff` drives over stdin/stdout.
`gba-core` never links mGBA, and `gba-diff` only spawns the `mgba-ref` process.
mGBA source is useful for understanding behavior, but this project's architecture differs; adapt ideas rather than copying files.
mGBA is MPL-2.0, so any copied file would carry that license.

## Build the reference

Requires cmake and a C compiler, which the dev shell provides:

```sh
direnv exec . python3 tools/prepare_mgba_reference.py roms/mgba-reference
```

The destination must be new, and outside the repository or inside the ignored `roms/`.
The script downloads mGBA 0.10.5 (`tools/mgba-reference.lock.json` pins its size and SHA-256), extracts it with path checks,
and builds a static library without frontends, OpenGL, scripting, debuggers, or optional libraries.
It writes `mgba-ref`, plus `mgba-perf` for speed comparison with this emulator's `--bench-frames` mode. Use `--tarball PATH` for an offline copy of the same tarball.
Build logs stay in the destination. Verified on Darwin arm64 and on Linux x86_64 (GCC 16) with Nix cmake 4.4.
On Linux the script omits two Clang-only warning flags and archives with `gcc-ar`, which mGBA's LTO objects need.

## Frame comparison

```sh
direnv exec . cargo run --locked --release -p gba-diff -- \
  --reference roms/mgba-reference/mgba-ref --rom roms/game.gba --save-type flash128 \
  --frames 600 --input 120:start:4,200:a:4 --out /tmp/gba-diff
```

After every frame (each VBlank), it compares EWRAM, IWRAM, palette RAM, VRAM, OAM, 30 readable I/O registers, and all
pixels at 5 bits per channel. It prints the first divergent frame in detail, then one summary line per later divergent
frame (`--max-report`). `--out` writes this emulator's frame, mGBA's frame, and a red difference mask as PPM files,
for the first divergent frame and for any frames listed with `--images`.
`--detail-frame FRAME[,...]` prints the same detail for later frames, listing up to 256 differing addresses per region
instead of the first six, so a residual byte on a late frame can be attributed. A listed frame without a difference
is reported as such.
`--ignore START-END[,...]` skips address ranges. Inputs use `FRAME:BUTTON:HOLD`, counted from frame 0.
Exit status is 0 without divergence, 1 with one, and 2 for usage or setup errors.

mGBA chooses its own save type from its game database or on first access; `--save-type` applies only to this emulator.

`--rtc UNIX_SECONDS` attaches the cartridge RTC on both sides and starts it at that time (UTC). Both clocks then run on
emulated time, not the host clock: mGBA's fake-epoch source gives `UNIX + frames * 280896 / 16777216` seconds, rounded
down, with frames counted from the cartridge entry, and this emulator advances its RTC at each frame boundary to the
same second. Without it, mGBA still attaches an RTC for games its database lists (Emerald, Ruby, Sapphire) and reads
the host's local time, while this emulator runs without one. The shim's `rtc` command arrived after the first builds;
rebuild with `tools/prepare_mgba_reference.py` if `mgba-ref` answers `err unknown command`.

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

Frame comparison also re-aligns after the game's first `RegisterRamReset` (`SWI 0x01`). mGBA runs that call as a
high-level function of a few dozen cycles, while this emulator's firmware clears RAM with real stores (a full reset
takes over three frames); neither cost is hardware-verified. Both sides run in instruction lockstep to that call, then
mGBA's display position is moved to ours again. Without this, every later frame differs only by that offset.
The search covers the first 1,000,000 instructions. A ROM that makes no such call, such as the mGBA test suite, is
restored to its cartridge-entry state, so both sides start the frame comparison from the same point.

The reference is built with mGBA's 16-bit (BGR555) renderer. Its default 32-bit renderer blends 8-bit channels, which
rounds differently from hardware's 5-bit blending by one step in some fades; the 16-bit renderer matches this emulator.

## Findings so far

Re-measured 2026-11-13 after the BIOS service, register-reset, and DMA start-delay changes, on the same host:

- Frame comparison with `--ignore 0x03007c00-0x03007fff`: FireRed and Emerald differ in `DMA1CNT_H` and
  `DMA2CNT_H`, the sound-FIFO channels whose destination-control bits mGBA sets itself, plus a few IWRAM bytes per
  frame. Those bytes were attributed on 2026-11-14 (below).

Attributed 2026-11-14 and re-measured 2026-11-15 over 600 frames. With the known noise ignored, FireRed settles at 6
IWRAM bytes per frame, all from three causes that are configuration or sampling differences rather than behavior:

| FireRed | Emerald | Cause |
| --- | --- | --- |
| `0x03000004` | `0x0300081c` | The game's copy of `DISPSTAT` in its register shadow buffer, a read-modify-write that keeps the read-only status flags. mGBA read it inside HBlank, this emulator just before. Sampling phase, not state. |
| `0x03007420–21`, `0x03007434` | `0x03007850`, `0x03007864` | Flash-library pointers chosen by flash ID. This emulator reports Macronix `0x09C2` for 128 KiB, mGBA reports Sanyo `0x1362`; both chips exist in real cartridges. |
| `0x03007476–77` | `0x030078a6–a7` | The game's serial handshake with no cable attached. This emulator shifts in 1s and reads `SIODATA32` as `0xffffffff`; mGBA without a link driver leaves the written value. The hardware open-line value is not verified here. |

Emerald's clock bytes (`0x03000db8–dcc`) match with `--rtc`. Emerald still diverges from frame 14: its intro setup
finishes one frame later here (first frame-counter increment at line 167, mGBA at line 136), so the fade and later the
per-frame tasks and RNG run one frame out of step, about 3,000 IWRAM bytes by frame 600.

Traced on 2026-11-16 by aligning both instruction streams outside the BIOS and comparing cycle counts. The setup reaches
the counter about 38,000 cycles later here. mGBA finishes before the VBlank and this emulator just after it. Two
differences make up most of that gap, and both are mGBA charging less than the bus allows:

- `LZ77UnCompVram`, called twice from ROM with the prefetch buffer on, costs 17,254 cycles more here. mGBA's
  high-level BIOS runs its prefetch-stall model on the decompressor's own VRAM accesses, as if ROM code issued them,
  so each one counts as -2 cycles. Predicted from the token counts, 6,216 and 10,984 cycles, against 6,252 and
  11,002 measured. With prefetch off inside mGBA's decompressor (a scratch build), the calls match within 36 and
  18 cycles. The real firmware runs from the BIOS, where the Game Pak prefetch does not apply.
- A Thumb copy from ROM to EWRAM at `0x082e93ee` (`LDMIA r3!,{r0}`, `STMIA r1!,{r0}`, 4 pairs per pass, 260 passes)
  costs 19 cycles per pair here and 9 in mGBA, about 10,000 in all. A word read from ROM at WAITCNT `0x4017` (4 + 2
  cycles) and a word write to EWRAM (3 + 3 cycles) need at least 12 bus cycles with no opcode fetches at all, so 9 is
  too low. mGBA credits prefetch hits during the EWRAM store that the next ROM data read cancels. The hardware cost lies
  between that minimum and this emulator's; the prefetch queue rules here are nominal
  ([Game Pak opcode prefetch](hardware/cpu.md#game-pak-opcode-prefetch)), and no hardware test here decides this case.

Neither is changed here: the first is an mGBA artifact, and the second needs hardware evidence.
- Instruction lockstep stops at the same instruction 1021 in both games: the immediate DMA3 readback below.
  Cumulative drift is +947,772 cycles, about 945,000 of it the `RegisterRamReset` call.

The earlier measurements, kept for history, were made 2026-11-13 before any timing changes:

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
  Deferring immediate DMA until after the next instruction matched mGBA here, but cost 194 memory and 56 timing results
  in the mGBA suite, whose DMA results are read right after each copy. mGBA's actual rule is cycle-based: the transfer
  starts at the first instruction boundary at least 3 cycles after the enabling instruction began. That rule is now
  [modeled](hardware/dma.md) and raised the suite's timing results. FireRed's readback still differs, because mGBA
  charges a store's following ROM fetch to the next instruction (store 2 cycles, load 11) while this emulator charges it
  to the store (5 and 8). Adopting mGBA's attribution lowered the suite's memory and timing results, so it was not.

## Speed comparison

`mgba-perf` is built with the reference. Compare it with the emulator's own headless benchmark on the same ROM and
frame count, both without video rendering:

```sh
./roms/mgba-reference/mgba-perf -N -P -F 600 roms/game.gba
./target/release/gameboy-rust --rom roms/game.gba --bench-frames 600
```

With `-N`, `mgba-perf` prints `GAMECODE,FRAMES,MICROSECONDS,none`. Frames per second is `FRAMES / (MICROSECONDS / 1e6)`;
the real-time multiple divides that by 60. `--bench-frames` reports the same two values directly, plus machine steps and
nominal cycles per second. Both timings include emulated boot. They are host measurements, not accuracy results, and a
higher frame rate does not make one emulator more correct than the other.

`mgba-perf` enables idle-loop detection, which skips polling loops that this emulator executes instruction by
instruction. `-C idleOptimization=ignore` turns it off for a like-for-like comparison.

Measured 2026-11-14 on Darwin arm64 (Apple M3 Pro) in a release build, 600 frames each, three runs each, with video
rendering disabled on both sides:

| ROM | This emulator | mGBA, idle loops skipped | mGBA, `idleOptimization=ignore` |
| --- | ---: | ---: | ---: |
| Pokémon FireRed (flash128) | 381 fps (6.4×) | 5287 fps (88×) | 2720 fps (45×) |
| Pokémon Emerald (flash128) | 378 fps (6.3×) | 5568 fps (93×) | 2778 fps (46×) |
| mGBA test suite (SRAM) | 640 fps (10.7×) | 8170 fps (136×) | 8190 fps (137×) |

Runs varied by about 2%. `--bench-frames` enables this emulator's exact
[idle-loop fast-forward](hardware/timers-irq.md#idle-loop-fast-forward), which roughly doubles the two games (194 fps
without it) because both poll a RAM flag for VBlank. The suite ROM gains nothing from it (its speed is the same with
and without), and it ran 5% slower than the build before the fast-forward; in one binary, enabling it costs the suite about
1%, so most of that difference is code layout between builds. mGBA with idle-loop skipping is about 14 times faster
on the two games and 13 times faster on the suite ROM. The same games ran at 55 fps on 2026-11-13 (commit 9e8eb8d)
and at about 114 fps at f165ca7. Turning on video rendering costs this emulator about 10% to 30% (more with
fast-forward, since rendering is not skipped) and mGBA about 5–15%. Concurrent builds on the same host lower these
figures by 10% or more, so compare runs taken back to back.

## Limits

- Only the GBA is supported. mGBA's Game Boy core is built but not driven.
- No BIOS image is passed to mGBA. A real BIOS would make BIOS-internal comparison possible, but none is distributed here.
- Paths passed to `mgba-ref` must be UTF-8 without whitespace.
- Audio is not compared.
- Known noise: residue below the stack pointer (`0x03007c00–0x03007fff`) differs because the two BIOS implementations
  push different words, and mGBA sets the destination-control bits of sound-FIFO DMA control registers
  (`0x040000c6`, `0x040000d2`) itself. Use `--ignore` for these when looking for behavior differences. The game-specific
  bytes in [Findings so far](#findings-so-far) (shadow `DISPSTAT`, flash ID, serial handshake) are noise of the same
  kind.
