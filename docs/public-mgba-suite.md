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
Passing tests are not logged. The suite writes a `<sub-suite> test: <test name>` header before each test and one
`<configuration>: Got <actual> vs <expected>: FAIL` line per failing result.

Pass counts come from memory. At this revision, the `suites` table of 14 `struct TestSuite` pointers is at ROM `0x0803d868`.
Each struct holds `passes` and `totalResults` pointers at offsets 20 and 24.
A sub-suite is finished when `totalResults` is nonzero and has stopped changing.
The video sub-suite is visual only and reports no counts.

The suite also probes mGBA's debug port at `0x04fff780`. On hardware that address is unused I/O, read as open bus.
This emulator does not implement mGBA's debug port, so the suite runs in its hardware mode.

## Scoreboard tool

`crates/gba-diff/examples/mgba_suite_scoreboard.rs` reads those counters without a local probe.
It boots the ROM once per sub-suite, presses Down to reach it, presses A, and polls `totalResults` until it stops
changing for `--settle` frames (default 240):

```sh
direnv exec . cargo run --locked --release -p gba-diff --example mgba_suite_scoreboard -- \
  --rom roms/mgba-suite/suite.gba
```

It prints one row per sub-suite and the `: FAIL` lines the suite wrote to SRAM, each prefixed with its test name
(`<test name> | <configuration>: Got ...`). `--log PATH` writes every failure line to a file.
It exits 0 when every counted sub-suite settled.
`--frame-cap` (default 18000) bounds a sub-suite that never settles; that case exits 1, because a timeout is never a pass.
The tool reports the suite's own counters and adds no verdict of its own.

## Baseline on Darwin arm64

Measured 2026-11-13 in a release build, after the [SRAM wide-access](hardware/saves.md#sram) and
[unused-I/O open-bus](hardware/cpu.md#arm-unused-memory-data-reads) fixes that the suite first exposed.
Each sub-suite started from a fresh boot. These results came from a local probe written before the
[scoreboard tool](#scoreboard-tool) existed; the re-run below reproduces them.

| # | Sub-suite | Passed / results | Current | Stopped by |
| ---: | --- | ---: | ---: | --- |
| 0 | Memory | 43 / 44 so far | 1552 / 1552 | — |
| 1 | I/O read | 4 / 40 so far | 130 / 130 | — |
| 2 | Timing | 1660 / 2020 | 2020 / 2020 | — |
| 3 | Timers | 362 / 936 | 936 / 936 | — |
| 4 | Timer IRQ | 0 / 90 | 90 / 90 | — |
| 5 | Shifter | 140 / 140 | 140 / 140 | — |
| 6 | Carry | 93 / 93 | 93 / 93 | — |
| 7 | Multiply long | 52 / 72 | 72 / 72 | — |
| 8 | BIOS math | 20 / 30 so far | 615 / 615 | — |
| 9 | DMA | 12 / 12 so far | 1244 / 1244 | — |
| 10 | SIO read | 1 / 6 so far | 90 / 90 | — |
| 11 | SIO timing | 4 / 4 | 4 / 4 | — |
| 12 | Misc. edge | 4 / 12 | 8 / 12 | — |
| 13 | Video | visual only | visual only | — |

The current column is the latest local run. Since the baseline, the sub-suites that used to stop were fixed
(unused-memory writes, SRAM's 8-bit bus in DMA and block transfers, I/O read policy, misaligned BIOS copies, BIOS
math side effects, serial register reads), the multiply carry flag was modeled, and Div, Sqrt, ArcTan, and CpuFastSet
were matched to the firmware's measured [service timing](hardware/bios.md#service-timing). Immediate DMA now starts
3 cycles after the enabling instruction began (timing +32, misc. edge +2). Services now run in System mode with the
caller's IRQ mask, like the firmware; that changed the IntrWait wake path the Timers tests count (−9).

Earlier the same day, the column was measured after the [timer start delay](hardware/timers-irq.md)
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

## Scoreboard re-run with repository tooling

Measured 2026-11-13 in a release build with the scoreboard tool above, a 240-frame settle window, and one fresh boot
per sub-suite. It reproduces the Current column exactly:

| # | Sub-suite | Passed / results |
| ---: | --- | ---: |
| 0 | Memory | 1552 / 1552 |
| 1 | I/O read | 130 / 130 |
| 2 | Timing | 2020 / 2020 |
| 3 | Timer count-up | 936 / 936 |
| 4 | Timer IRQ | 90 / 90 |
| 5 | Shifter | 140 / 140 |
| 6 | Carry | 93 / 93 |
| 7 | Multiply long | 72 / 72 |
| 8 | BIOS math | 615 / 615 |
| 9 | DMA | 1244 / 1244 |
| 10 | SIO register R/W | 90 / 90 |
| 11 | SIO timing | 4 / 4 |
| 12 | Misc. edge | 8 / 12 |
| 13 | Video | visual only |

The recorded `: FAIL` lines list the remaining mismatches by test name. The numbers no longer depend on an
uncommitted local probe.

The table was re-measured on Linux x86_64 after the Timing fixes below; a ROM built there from the pinned revision
with the `devkitpro/devkitarm` image has the same SHA-256 as the Darwin build. Timing moved from 1824 to 2020:

- Div and ArcTan no longer reload a service frame they did not need (60 results); see
  [service timing](hardware/bios.md#service-timing).
- Opcode prefetch pauses during DMA and keeps its queue, with a one-cycle stall at one halfword phase, and a
  ROM-to-ROM transfer writes its first unit sequentially (136 results); see [DMA](hardware/dma.md).

Timers (587 to 936), Timer IRQ (44 to 90), and SIO timing (0 to 4) then followed from the
[request delay, timer stop latency, and reload latch](hardware/timers-irq.md#request-delay-stop-latency-and-reload-latch),
with the HALT wake cost and IntrWait's exit path fitted to them.

## Remaining failures

Every sub-suite now runs to completion. Every counted sub-suite except Misc. edge passes every result:

- Misc. edge "DMA Prefetch" now passes: the instruction after a DMA unit reads the unit's data as
  [open bus](hardware/dma.md#bus-value-after-a-transfer), and blanking transfers start 3 cycles after their edge.
- Misc. edge "H-blank bit start" still fails "Hblank" and "Flip" 1 to 3. These four expectations cannot be met with
  the pinned build. The test times C code, and this compiler places three `movs` instructions between the second
  `Halt` and its timer read (`0x03000118`–`0x0300011c`) that the first read does not have, so two identical wake
  paths read 1,235 cycles apart, not the expected 1,232. The polling loops behind "Flip" 3, 4, and 5 share one
  8-cycle period and identical code between them, so those results must agree modulo 8; the expected values are 5, 3,
  and 3 modulo 8, which no fixed instruction timing of this binary produces. The expectations were presumably recorded from a
  different build. "Flip" 4 to 6 pass. Running the same ROM in mGBA would confirm this; that has not been done here.
