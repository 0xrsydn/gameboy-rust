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

The initial sequential-only implementation invalidated history after all refill instructions.
The extension below now establishes target-pair history for successful refills into Thumb IWRAM.
The existing ARM/Thumb timing classifiers identify these refills for both timed and untimed CPU stepping.
Execution outside Thumb IWRAM still ends a sequential history sequence.
An accepted machine IRQ invalidates history. Failed DMA units preserve it.
The DMA extension below updates existing continuation lanes from successful IWRAM accesses.

This is not a full IWRAM bus model across all CPU states. ARM-target history and DMA-to-CPU ordering remain incomplete.
It is not a persistent instruction pipeline. Exact per-access device timing remains unverified.

## Thumb IWRAM refill extension

The [ARM7TDMI branch sequence](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/branch-and-branch-with-link) fetches the destination, then destination plus instruction width.
For Thumb destinations, those addresses are T and T+2.
[NanoBoyAdvance's refill helpers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) use that order.
Its execution loop then fetches T+4 before executing the first Thumb instruction.

After a successful refill instruction, we sample the two mapped destination halfwords if both remain in Thumb IWRAM.
The samples replace the old lane history and establish the next expected PC as T.
The next instruction's normal PC+4 sample updates one lane while the other retains the captured T+2 halfword.
The samples occur after data accesses and status restoration, so loaded PC values and saved Thumb state select the destination.

This covers actual ARM/Thumb BX, taken Thumb branches, BL suffixes, PC-writing Thumb operations, and ARM status-restoring returns.
The Thumb BL prefix alone does not refill.
Failed instructions retain the previous committed history. An unsupported or incomplete target sample does not fail the branch early.
Unmapped instruction targets still fail on the following fetch.
Cold direct startup and arbitrary PC changes do not manufacture a refill.

These are bus-history samples, not instruction-buffer contents or extra emulated cycles.
ARM and Thumb execution now use a separate instruction buffer. Cold buffer filling does not seed this bus history.
Changing T+2 after the branch does not replace its captured bus value; changing T+4 before arrival affects the later sample.
Those original tests verify sample ordering, not hardware-accurate self-modifying instruction execution.

The refill-only implementation invalidated history on successful DMA, including between refill and arrival.
The DMA extension below replaces that invalidation within existing continuations.
ARM-target refill history, exact pipeline timing, and sub-instruction DMA ordering remain incomplete.
No external emulator differential run or physical-hardware test was performed.

## DMA continuation extension

Two independent implementations route DMA through the same local IWRAM lane updates as CPU accesses:

- [ares bus dispatch](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) selects its IWRAM helpers for both CPU and DMA.
  [Its DMA controller](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/dma.cpp) performs a source read before its destination write.
- [jgenesis bus at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/bus.rs) routes `AccessCtx::DMA` through `read_iwram` and `write_iwram`.
  Both call `iwram_update_open_bus`, which changes only addressed lanes and drives the general bus from the local word.
  Its `try_progress_dma` performs source reads and destination writes in order.

Both run pending DMA before a CPU bus access. The access that resumes determines the subsequent bus value.
These source reviews support local lane effects, not a universal first-instruction DMA override.
They do not determine every hardware arbitration edge or validate our instruction-boundary scheduling.

Our bounded model applies those effects only when an existing Thumb IWRAM continuation has an expected next PC.
After a successful unit, the source and destination independently update their addressed IWRAM lanes.
DMA elsewhere leaves those lanes unchanged. A blocked source drives no IWRAM read lanes.
The destination sees the actual selected halfword, not an assumed duplicated full word.
The next PC+4 sample updates one halfword before the resumed instruction takes its snapshot.
This also applies between a completed refill and its first target instruction.

Only complete successful units commit history, preserving the project's atomic diagnostic policy.
Cold direct entry still has no expected continuation PC. DMA does not manufacture one, even after a word access.
An existing partially known continuation can gain lanes. IRQ entry and CPU discontinuities retain their invalidation rules.
General DMA open-bus reads, cross-state history, sub-instruction arbitration, and DMA during CPU internal cycles remain outside this subset.
This extension models bus lanes, not self-modifying instruction execution.
The later [ARM](arm-instruction-buffer.md) and [Thumb](thumb-instruction-buffer.md) instruction buffers separately retain instructions across DMA.

No physical-hardware measurements or external emulator differential runs were performed.
All regression programs are original, with a separate byte-array lane reference in the DMA matrix tests.

## Original regression coverage

- Both code alignments, IWRAM mirrors, and physical RAM wrap.
- Captured halfwords that remain unchanged by host inspection or later host writes.
- Byte/halfword/word data reads and writes, sign extension, load rotation, and all processor modes.
- Other-region accesses that leave IWRAM lanes unchanged.
- Multiple-register transfers, final-word retention, and writeback.
- Unknown lanes, establishment of known history, staged-access rollback, and retry.
- Taken and untaken branches, PC writes to fallthrough, discontinuities, IRQ entry, and DMA success/failure.
- ARM/Thumb source regions, both target alignments, refill sample ordering, stack/block returns, and saved User/System banks.
- Actual SWI/IRQ return handlers, failed refills, unmapped targets, separate BIOS history, and DMA between refill and arrival.
- DMA source/destination regions and alignments, word/halfword widths, mirrors, blocked-source lane selection, and other-region isolation.
- Channel preemption, completed-unit order, DMA/instruction failure retention, partial/cold continuations, and unchanged other CPU snapshot rules.
- Equal timed/untimed CPU results and unchanged nominal data/device costs.

## Validation result

The sequential-history regressions reproduced the old unsupported-read failures before implementation.
The refill regressions also failed on the prior invalidation-only path, then passed with target-pair sampling.
The DMA continuation regressions reproduced unsupported-read failures under unconditional DMA invalidation, then passed with local lane updates.
Workspace and core/demo tests pass on Darwin arm64 in debug and release builds.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes also pass.
Public ARM, Thumb, memory, and BIOS reports match their previous passing results exactly.
Debug and release reports also match for the same fixture paths.

Those public suites remain regression checks, not independent IWRAM-history conformance tests.
