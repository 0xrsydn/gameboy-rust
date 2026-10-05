# IWRAM Thumb bus history

## Finding

Internal work RAM (IWRAM) retains its own bus lanes. A previous CPU load result is not a universal replacement.
An IWRAM byte, halfword, or word access updates only the addressed lanes of that latch.
A Thumb fetch updates one halfword. Reads and writes elsewhere do not replace the IWRAM latch.
This distinction matters when Thumb code reads another memory region before an unused-memory read.

For sequential Thumb execution at P, the fetch at P+4 updates:

- Bits 0–15 when P is word-aligned.
- Bits 16–31 when P has only halfword alignment.

The other halfword retains its previous IWRAM value.
Without intervening IWRAM data accesses, consecutive fetches produce GBATEK's usual P+2/P+4 combination.
Using current bytes at P+2 would lose observed history and ignore data-access changes.

## Evidence

- [GBATEK](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm) documents the two Thumb IWRAM lanes and warns about overwritten history.
- [mGBA issue 1575](https://github.com/mgba-emu/mgba/issues/1575) reports hardware-based alignment expectations and read/write effects on IWRAM open bus.
- [ares IWRAM access implementation](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/memory.cpp) retains separate IWRAM lanes for byte, halfword, and word reads and writes.
  Its [bus implementation](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) distinguishes this latch from the general data bus and debugger access.
- [jgenesis issue 676](https://github.com/jsgroth/jgenesis/issues/676) identifies failures after byte/halfword reads from other regions.
  The [correction at fab6e2cc](https://github.com/jsgroth/jgenesis/commit/fab6e2ccc60e492dd68b7f1e927b0829a6d80195) adds a separate IWRAM latch.
  The commit reports that its openbuster tests pass. That report is upstream evidence, not a test run by this project.

These sources support a separate lane-retaining latch, rather than a copy of the last CPU destination register.
Our implementation and synthetic programs are original. No external source code or test ROM was imported.
No physical-hardware measurement or external emulator differential run was performed here.

## Bounded implementation

`memory/iwram_bus.rs` records lane values and a known-bit mask during consecutive Thumb instructions in IWRAM.
Before execution, the mapped PC+4 halfword updates its addressed lanes.
During execution, successful IWRAM bus reads and writes update the staged lanes at their actual aligned addresses and widths.
The latch sees raw bus data before CPU load rotation and sign extension.
Other-region accesses and host inspection do not change it.

A completed sequential instruction commits the staged history for the next exact virtual PC.
A failed instruction discards all staged changes and preserves the last committed history.
The existing whole-instruction diagnostic policy still applies; this is not hardware data-abort behavior.
All unused-memory reads within one instruction use its entry snapshot.

The model starts with unknown lanes. An unused-memory load requires a complete known word, even for a narrower load.
Ordinary instructions and mapped data accesses can establish history without requiring a prior complete word.
Consecutive halfword fetches or an IWRAM word access can make every lane known.
Missing lookahead and 16 MiB region-crossing fetches retain diagnostics.

Successful branches and other refill instructions invalidate continuation history, even when the target equals the next sequential address.
The existing Thumb timing classifier identifies these refills for both timed and untimed CPU stepping.
Execution outside Thumb IWRAM also ends this bounded history sequence.
An accepted machine IRQ and each successful DMA unit invalidate the history. Failed DMA units preserve it.
The implementation does not guess refill or DMA latch contents.

This is not a full IWRAM bus model across all CPU states. ARM access history, refill fetches, and DMA-to-CPU ordering remain incomplete.
It is not a persistent instruction pipeline. Self-modifying instruction execution and exact per-access device timing remain unverified.

## Original regression coverage

- Both code alignments, IWRAM mirrors, and physical RAM wrap.
- Captured halfwords that remain unchanged by host inspection or later host writes.
- Byte/halfword/word data reads and writes, sign extension, load rotation, and all processor modes.
- Other-region accesses that leave IWRAM lanes unchanged.
- Multiple-register transfers, final-word retention, and writeback.
- Unknown lanes, establishment of known history, staged-access rollback, and retry.
- Taken and untaken branches, PC writes to fallthrough, discontinuities, IRQ entry, and DMA success/failure.
- Equal timed/untimed CPU results and unchanged nominal data/device costs.

## Validation result

The sequential-history regressions reproduced the old unsupported-read failures before implementation.
Workspace and core/demo tests pass on Darwin arm64 in debug and release builds.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes also pass.
Public ARM, Thumb, memory, and BIOS reports match their previous passing results exactly.
Debug and release reports also match for the same fixture paths.

Those public suites remain regression checks, not independent IWRAM-history conformance tests.
