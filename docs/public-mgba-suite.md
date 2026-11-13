# Pinned mGBA test suite

The [mGBA test suite](https://github.com/mgba-emu/suite) is a public homebrew ROM with hardware-verified expectations for memory, I/O reads, timing, timers, DMA, serial, and CPU edge cases.
It is a broad accuracy scoreboard, not a pass/fail checkpoint like the pinned [ARM](public-arm-tests.md) and [Thumb](public-thumb-tests.md) ROMs.
Nothing from the suite is committed here: no source, ROM, or build output.

## Provenance

- Repository: `mgba-emu/suite`, MIT license, copyright Jeffrey Pfau and contributors.
- Revision: `e6942030d25ffe3ba76c72b73a86da073ec857cc` (2026-07-08, 94 commits).
- Upstream publishes no ROM, so it is built locally with devkitARM, as upstream's `docker-build.sh` does.
- Build image: `devkitpro/devkitarm@sha256:116afba8df8453961de2936ffab20dd441edf4d682856c1ec8b0e53d7ed0bbf5` (arm64).
- Output: `suite.gba`, 524,288 bytes, SHA-256 `8cf68cd31c5468a70aca8a1c5b63dc372fe755c40b446cf6aa6d0fc6e3a1f035`.
  Two clean builds from the same revision and image produced identical bytes.

## Build

Use a destination outside the repository or inside the ignored `roms/` directory:

```sh
git clone https://github.com/mgba-emu/suite.git roms/mgba-suite
git -C roms/mgba-suite checkout e6942030d25ffe3ba76c72b73a86da073ec857cc
docker run --rm -v "$PWD/roms/mgba-suite:/root/suite" -w /root/suite \
  devkitpro/devkitarm@sha256:116afba8df8453961de2936ffab20dd441edf4d682856c1ec8b0e53d7ed0bbf5 make
shasum -a 256 roms/mgba-suite/suite.gba
```

A different revision or image can produce a different ROM; the addresses below then no longer apply.

## How results are read

The ROM shows a menu of 14 sub-suites. Down moves the cursor, and A runs the selected sub-suite.
It needs `--save-type sram`: at startup it clears SRAM, then writes a plain-text log of failures there (`savprintf`).
Passing tests are not logged. Each failure line reads `<name>: Got <actual> vs <expected>: FAIL`.

Pass counts come from memory. At this revision, the `suites` table of 14 `struct TestSuite` pointers is at ROM `0x0803d868`.
Each struct holds `passes` and `totalResults` pointers at offsets 20 and 24.
A sub-suite is finished when `totalResults` is nonzero and has stopped changing.
The video sub-suite is visual only and reports no counts.

The suite also probes mGBA's debug port at `0x04fff780`. On hardware that address is unused I/O, read as open bus.
This emulator does not implement mGBA's debug port, so the suite runs in its hardware mode.

## Baseline on Darwin arm64

Measured 2026-11-13 in a release build, after the [SRAM wide-access](hardware/saves.md#sram) and
[unused-I/O open-bus](hardware/cpu.md#arm-unused-memory-data-reads) fixes that the suite first exposed.
Each sub-suite started from a fresh boot. Repository tooling for this run does not exist yet; the results came from a local probe.

| # | Sub-suite | Passed / results | Current | Stopped by |
| ---: | --- | ---: | ---: | --- |
| 0 | Memory | 43 / 44 so far | 1511 / 1552 | — (misaligned BIOS copies remain) |
| 1 | I/O read | 4 / 40 so far | 4 / 40 so far | CPU read of unused register slot `0x04000058` |
| 2 | Timing | 1660 / 2020 | 1680 / 2020 | — |
| 3 | Timers | 362 / 936 | 596 / 936 | — |
| 4 | Timer IRQ | 0 / 90 | 44 / 90 | — |
| 5 | Shifter | 140 / 140 | 140 / 140 | — |
| 6 | Carry | 93 / 93 | 93 / 93 | — |
| 7 | Multiply long | 52 / 72 | 52 / 72 | — |
| 8 | BIOS math | 20 / 30 so far | 20 / 30 so far | Undefined instruction in the BIOS replacement's argument trap |
| 9 | DMA | 12 / 12 so far | 1244 / 1244 | — |
| 10 | SIO read | 1 / 6 so far | 1 / 6 so far | CPU read of unmodeled `RCNT` at `0x04000134` |
| 11 | SIO timing | 4 / 4 | 0 / 4 | — |
| 12 | Misc. edge | 4 / 12 | 4 / 12 | — |
| 13 | Video | visual only | visual only | — |

The current column was measured on the same day after the [timer start delay](hardware/timers-irq.md)
(timing +20, timers +42, Timer IRQ +44) and the [minimal BIOS IRQ dispatch](hardware/bios.md) (timers +192).
All 20 calibration results now pass. SIO timing regressed: each normal-mode case now reads exactly two cycles low
(for example `0x277` against `0x279`). That test times a serial transfer through `SWI Halt`, IRQ dispatch, and the
handler in this emulator's replacement BIOS, whose IRQ dispatch is already known to be slower than hardware (Timer IRQ
read `0x56` where hardware reads `0x51`). Its earlier exact match therefore combined offsetting errors. With the
minimal dispatch, Timer IRQ reads `0x4e` and SIO timing is ten cycles low: interrupts are now taken too early. Hardware
delays IRQ entry after a request; mGBA models seven cycles (`GBA_IRQ_DELAY`). A [delay sweep](research/irq-delay.md)
found no clear optimum yet, because the replacement BIOS's IntrWait and Halt paths also differ from hardware.

"So far" means a core diagnostic stopped the sub-suite before it finished; later tests did not run.
Treat these numbers as a dated snapshot, not a claim about any game.

## First failures worth investigating

- DMA reads from unused memory (open bus for DMA) stop the memory and DMA sub-suites.
- An unused register slot below `0x04000400` stops the I/O-read sub-suite; GBATEK notes some of them read as zero.
  Reading the unmodeled `RCNT` register stops the SIO-read sub-suite.
- The BIOS math sub-suite reaches an unimplemented path in the original BIOS replacement.
- Timer IRQ fails every result, and every timing calibration result is two cycles high. They may share a cause; this is unverified.
- I/O read expects write-only scroll registers to return open bus (`0xDEAD` in the test) and unused BGxCNT bits to read back.
