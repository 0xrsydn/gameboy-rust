# Differential testing against Mesen 2

`gba-diff` can use [Mesen 2](https://github.com/SourMesen/Mesen2) as a second reference beside [mGBA](mgba-reference.md).
Mesen 2 is an accuracy-focused multi-system emulator with its own GBA core. Like mGBA, it is a reference, not ground
truth. When mGBA and Mesen agree and this emulator differs, the difference is very likely ours. When they disagree,
hardware-verified results such as the [mGBA test suite](public-mgba-suite.md) decide.

Nothing from Mesen is committed here. A script builds Mesen's GBA core locally from a pinned source tarball and links it
into `mesen-ref`, a small original C++ program (`tools/mesen-ref/`) that speaks the same stdin/stdout protocol as
`mgba-ref`. `gba-core` never links Mesen, and `gba-diff` only spawns the process.
Mesen is GPL-3.0-only. The shim's source is original, but the linked `mesen-ref` binary is a GPL-3.0-only combined work:
keep it local and do not commit or distribute it. Do not copy Mesen source files into this repository.

## Why it complements mGBA

Mesen has no high-level BIOS, so `mesen-ref` loads this emulator's original firmware (`bios::image()`) and both
emulators power on at the reset vector and run the same boot code. That removes the two largest sources of noise in the
mGBA comparison:

- No start alignment. mGBA starts at the cartridge entry on its own approximation of the BIOS exit, and `gba-diff` moves
  its display clock to ours. Against Mesen both sides reach the cartridge entry the same way.
- No BIOS-cost gap. mGBA runs services such as `RegisterRamReset` as high-level calls of a few dozen cycles, against
  about 948,000 cycles in this firmware. Against Mesen both sides run the same instructions, so BIOS calls compare
  directly and `gba-diff` does not re-align after the game's first `RegisterRamReset`.

## The pinned source

The tarball is the [`Mesen2-gba-headless`](https://github.com/mechanize-work/Mesen2-gba-headless) fork at commit
`bcfdd25` (`tools/mesen-reference.lock.json` pins its size and SHA-256). It comes from the
[GBA Eval](https://github.com/mechanize-work/gba-eval) benchmark, which grades emulators against it. The fork is a trim
of upstream commit `fabc9a6` (2025-07-15) to a single-threaded, GBA-only static library: other cores, netplay, threads,
the debugger UI, and the NTSC filter are deleted. Its changes inside `Core/GBA/` are five files, +5/−34 lines: an inline
video decode instead of a decoder thread, the NTSC filter branch, and an RTTI-free cast in the unused debugger. The
CPU, memory, prefetch, DMA, timer, APU, and cartridge sources are unchanged. Upstream Mesen 2 was archived on 2026-06-04
(successor: [MesenCE](https://github.com/nesdev-org/MesenCE)) with no GBA change after `fabc9a6`; checked 2026-11-21.

## Build the reference

Requires a C/C++ compiler, which the dev shell provides:

```sh
direnv exec . python3 tools/prepare_mesen_reference.py roms/mesen-reference
```

The destination must be new, and outside the repository or inside the ignored `roms/`. The script downloads and verifies
the tarball, extracts it with path checks, compiles the GBA core (about 130 sources, in parallel) into
`libmesen-gba.a`, and links `mesen-ref`. Use `--tarball PATH` for an offline copy of the same tarball. Build logs stay in
the destination. Verified on Darwin arm64 with Nix clang 21; Linux is untested.

## Use

Pass `mesen-ref` where `mgba-ref` would go. `gba-diff` asks the shim for its `name` and adapts:

```sh
direnv exec . cargo run --locked --release -p gba-diff -- \
  --reference roms/mesen-reference/mesen-ref --rom roms/game.gba --save-type flash128 --frames 600

direnv exec . cargo run --locked --release -p gba-diff -- \
  --reference roms/mesen-reference/mesen-ref --rom roms/game.gba --trace 2000000
```

Frame comparison, `--ignore`, `--detail-frame`, `--images` (written as `frameNNNNN-mesen.ppm`), and instruction lockstep
work as described for [mGBA](mgba-reference.md). Lockstep still skips instructions inside the BIOS, as against mGBA,
even though both sides now run the same firmware there.

`--bios IMAGE` runs another 16 KiB firmware on both sides instead of this emulator's, such as the
[GBA Eval](gba-eval.md) stub; frames then count from power-on on both sides. `--eval` scores frames as that benchmark
lines them up; see [Local scoring](gba-eval.md#local-scoring).

Differences from the mGBA path:

- `--rtc` is rejected. Mesen's RTC follows the host clock, so `mesen-ref` disables it for reproducible runs; RTC games
  such as Emerald run without one on both sides when `--rtc` is not given.
- Mesen chooses its save type from the ROM's library markers (`SRAM_V`, `EEPROM_V`, `FLASH1M_V`, ...), falling back to
  SRAM. Without `--save-type`, `gba-diff` gives this emulator the device its own detection picks (`none` forces none).
- `--no-align` has no effect.

## Shim details

`load ROM BIOS eval` instead configures Mesen as GBA Eval's reference does: Mesen's own boot skip, its own display
start, and keys taken at its VBlank input poll.

`mesen-ref` answers `name` with `ok mesen2`; `mgba-ref` has no such command and answers with an error, which `gba-diff`
takes to mean mGBA. `load ROM BIOS` requires the 16 KiB image. `write16`, `video`, and `rtc` answer
`err unsupported by mesen-ref`, because they only exist to align mGBA's start and clock.

To match `mgba-ref` and this emulator, the shim:

- reports r15 as the next instruction's address plus one instruction width, mGBA's pipelined convention, which
  `gba-diff` already normalizes;
- applies `keys` at once by polling Mesen's input immediately (Mesen otherwise polls at each VBlank);
- disables Mesen's frame skipping, which drops rendering of frames that finish within 15 ms of the previous one;
- powers on with zeroed RAM and without Mesen's boot skip, in a fresh temporary home folder per load, so that no
  battery save carries over between runs;
- starts Mesen's display at this emulator's power-on position (line 0, at the cycle Mesen's clock has reached, below
  one line). Mesen powers its display on at line 225 (upstream commit `d473d0e`, "Improved PPU timings", which fixed
  timing tests run through the official BIOS). With this firmware neither start is anchored to hardware, and the
  three-line offset otherwise shows up as a `VCOUNT` difference in every game that polls it;
- reports the PPU's finished RGB555 frame directly, without Mesen's color filters;
- sends Mesen's log output to stderr, so it cannot corrupt the reply stream.

A frame ends where Mesen's PPU starts line 160, the same VBlank this emulator reports.

## Findings so far

Measured 2026-11-21, with this emulator at `5d059b3`:

- mGBA test suite ROM (`--save-type sram`): the first six frames match exactly, from power-on through the firmware boot,
  in all compared memory, I/O, and pixels. Lockstep matches 260,807 instructions with at most 3 cycles of drift. It then
  diverges in a Thumb loop that stores to SRAM (`0x0e000000`): per-instruction costs there differ by 2 cycles in both
  directions, and this emulator takes an IRQ a few instructions earlier. SRAM store timing is already an open question
  against mGBA (see [Findings so far](mgba-reference.md#findings-so-far)).
- Pokémon FireRed (local dump, `--save-type flash128`): lockstep stopped at instruction 1021 with drift of only +4
  cycles (against mGBA, +947,772, almost all of it `RegisterRamReset`). The divergence was the immediate DMA3 readback
  documented for mGBA: Mesen, too, still reported the enable bit (`0x84000000`) where this emulator had finished the
  transfer (`0x04000000`). Frame comparison first differs at frame 4, in `IF`.

On 2026-11-22 this emulator adopted Mesen's immediate-DMA start rule (see [DMA](hardware/dma.md)), with no change in
the mGBA suite. FireRed and Emerald lockstep then passed the readback and stopped at instruction 1128, where a `VCOUNT`
read returned line 89 here and line 86 in Mesen. That was the power-on display position: at the first cartridge
instruction Mesen was on line 225 and this emulator on line 0, and the offset stayed constant. With the shim's start
alignment (above):

- FireRed and Emerald lockstep runs 2,438 instructions. From instruction 1,771 on, a taken short forward Thumb branch
  in a ROM shift-and-subtract division loop (`bcc` over one instruction, WAITCNT `0x4014` with prefetch on) costs 2
  cycles less here than in Mesen. Each pass adds 2 cycles until a `VCOUNT` polling loop reads line 104 here and 105 in
  Mesen. Both prefetch models deliver only from the queue head; the difference lies in when a halfword is ready around
  the branch, and the two emulators charge fetches to different instructions (Mesen after each instruction, this
  emulator before), so per-instruction counts cannot settle it. Neither model is hardware-verified here.
- Frame comparison of both games first differs at frame 4 in `IF` only: Mesen raises the VBlank request one cycle
  after line 160 begins (`SetDelayedIrqSource`), after its frame has ended, so `gba-diff` samples it before the bit is
  set. Later frames differ in two IWRAM bytes.
- The mGBA test suite ROM matches through frame 8 (previously 6); lockstep is unchanged at 260,806 instructions.

Measured 2026-11-22, with this emulator at `f077b46`:

- On the [mGBA test suite](public-mgba-suite.md#mgba-0105-and-mesen-2-on-the-same-rom), Mesen passes every Memory,
  I/O read, Timing, Timer IRQ, Multiply long, and DMA result, which mGBA 0.10.5 does not. It loses to this emulator on
  timer count-up, SIO, and one Misc. edge case, and fails the same four Misc. edge expectations, with the same values.
- How Mesen starts an immediate DMA (`Core/GBA/GbaDmaController.cpp`): the enabling write sets a start time 2 cycles
  later (3 for HBlank, 3 for sound FIFOs, 5 for video capture). The transfer begins at the first cycle of a bus
  access or idle cycle at or after that time, so it can start partway through an instruction, with an idle cycle
  before and after the transfer.
- In FireRed's readback (`0x080006a4`–`0x080006a8`), Mesen charges the `STR` to `DMA3CNT` 2 cycles and the earlier
  ROM literal load 11, as mGBA does, where this emulator charges 5 and 8. The following `LDR r0,[r0]` reads
  `0x84000000`, and its cost includes the whole transfer (2,569 cycles): the transfer began in that load's internal
  cycle, after its read. [mGBA findings](mgba-reference.md#findings-so-far) record that adopting mGBA's attribution here
  lowered this emulator's suite results. Mesen pairs the same attribution with full Timing and DMA results, so the
  attribution alone does not rule those results out. This is not yet investigated further.

## Limits

- Only Darwin arm64 is verified.
- Audio is not compared, and the shim does not expose Mesen's samples.
- Lockstep does not yet compare inside the BIOS, although both sides run the same firmware.
- `gba-diff` paths passed to the shim must be UTF-8 without whitespace; the firmware image goes through a temporary file.
