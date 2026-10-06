# Persistent IWRAM bus history

## Finding

Internal work RAM (IWRAM) retains its own bus lanes, independently of the CPU's instruction state and executing region.
An IWRAM byte, halfword, or word access updates only the addressed lanes of that latch.
ARM instruction fetches drive words. Thumb instruction fetches drive halfwords.
Reads and writes elsewhere do not replace the IWRAM latch.
Neither a prior CPU destination register nor a DMA channel value is a universal replacement.

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
  Its helpers update the same latch without testing CPU state, executing PC, or sequential continuation.
  Its [bus implementation](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) selects those helpers for CPU and DMA accesses and isolates debugger reads.
- [jgenesis issue 676](https://github.com/jsgroth/jgenesis/issues/676) identifies failures after byte/halfword reads from other regions.
  The [correction at fab6e2cc](https://github.com/jsgroth/jgenesis/commit/fab6e2ccc60e492dd68b7f1e927b0829a6d80195) adds a separate IWRAM latch.
  The commit reports that its openbuster tests pass. That report is upstream evidence, not a test run by this project.
- [NanoBoyAdvance's CPU loop and refill helpers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) fetch the new instruction before execution and sample target pairs in instruction-width order.

These sources support local bus ownership rather than a latch restricted to consecutive Thumb instructions.
Our implementation and synthetic programs are original. No external source code or test ROM was imported.
No physical-hardware measurement or external emulator differential run was performed here.

## Implementation and cold fills

`memory/iwram_bus.rs` stores persistent lane values and a known-bit mask.
There is no expected PC or CPU-state tag on this latch. The instruction buffer retains its own PC/state tags.
Every instruction stages changes from the previously committed lanes.
The [shared fetch samples](shared-fetch-samples.md) drive successful IWRAM fetches at their actual address and instruction width.
Successful IWRAM data accesses then drive raw bus values before load rotation or sign extension.
CPU code can run in BIOS, ROM, EWRAM, or IWRAM without changing that rule.

A retained pipeline drives only its newly fetched lookahead. It never replays current/decode bus samples.
A cold or discontinuous pipeline supplies current, next, and lookahead samples in order.
A valid Thumb IWRAM cold fill therefore establishes both lanes before the first data read.
This follows actual samples already performed by the interpreter; it is not an invented previous P+2 memory read.
Debugger invalidation clears only instruction retention, but the following cold fill drives its new samples.

A successful instruction commits its staged lanes regardless of its resulting state, PC, or region.
A failed instruction discards every staged change, including cold-fill samples.
Successful PC-writing instructions then apply target fetches after their data accesses.
Other-region execution, BIOS IRQ entry, and unavailable targets do not erase known local lanes.
Host inspection and setup never drive the latch.

Initial lanes remain unknown until observed accesses establish them.
Byte/halfword accesses may establish only part of the word; a word access establishes all lanes.
All unused-memory reads within an instruction still use one entry snapshot.
A Thumb fetch crossing a 16 MiB region boundary leaves that instruction's unused-memory snapshot unsupported.
Successful IWRAM accesses still update the local latch independently of this conservative snapshot policy.

## Thumb IWRAM refill extension

The [ARM7TDMI branch sequence](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/branch-and-branch-with-link) fetches the destination, then destination plus instruction width.
Thumb destinations use T and T+2; ARM destinations use T and T+4.
The same captured samples fill the instruction buffer and drive local IWRAM lanes, with no duplicate memory reads.
Each successful IWRAM sample drives its lanes independently. Other-region or unavailable samples leave them unchanged.

A complete Thumb IWRAM target pair replaces both halfwords.
The first target instruction fetches T+4, replacing one halfword while retaining the captured T+2 lane.
An ARM IWRAM target pair leaves T+4's word in the latch.
Data loads and status restoration finish first, so saved instruction state determines the refill width.

This covers BX, taken branches, BL suffixes, PC writes/loads, and status-restoring returns.
The Thumb BL prefix alone does not refill.
A branch with an unavailable target can complete; the later instruction fetch retains its strict diagnostic.
A failed branch or return does not commit data or target lane changes.
IRQ vector samples in BIOS leave IWRAM unchanged, but an IRQ handler's IWRAM data access updates it normally.

## DMA continuation extension

Two independent implementations route DMA through the same local IWRAM lane updates as CPU accesses:

- [ares DMA](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/dma.cpp) performs a source read before its destination write through the shared bus dispatch above.
- [jgenesis bus at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/bus.rs) routes `AccessCtx::DMA` through `read_iwram` and `write_iwram`.
  Both call `iwram_update_open_bus`. Its DMA controller performs source reads and destination writes in order.

Our instruction-boundary scheduler commits source lanes, then destination lanes, only after a complete unit succeeds.
A blocked source below work RAM drives no IWRAM read lanes.
A halfword destination drives its actual selected half, not both halves of a duplicated channel word.
DMA can establish local lanes before any CPU instruction, without an expected continuation PC.
Failed DMA units preserve the previous committed latch.

A retained Thumb pipeline resumes with one PC+4 sample; a cold pipeline resumes with its full fill sequence.
These actual fetches determine how much DMA history remains visible.
DMA channel values never overwrite CPU instruction-buffer slots or protected BIOS history automatically.
Sub-instruction arbitration and DMA during CPU internal cycles remain unmodeled.

## Limits and original coverage

This is persistent local IWRAM history, not a complete general-bus implementation or cycle-accurate pipeline.
ARM unused-memory reads still use their supported PC+8 snapshot.
BIOS protection remains separately retained and image-derived.
General DMA open bus, unused/write-only I/O, disabled RAM, exact region-crossing snapshots, and per-access timing remain incomplete.
Nominal CPU/DMA costs do not change.

Original tests cover:

- Cold fills, both Thumb alignments, mirrors, physical RAM wrap, and entry snapshots.
- ARM word fetches and ARM/Thumb target pairs, without replaying retained instruction slots.
- ARM byte/halfword/word reads and stores from EWRAM, plus Thumb data access from EWRAM.
- Preserved lanes through state changes, non-IWRAM execution, IRQ entry, and BIOS handler fetches.
- DMA before CPU startup, source/destination ordering, channel preemption, and blocked-source lane selection.
- Failed sequential instructions, failed cold fills, failed refills, and failed DMA units.
- Separate BIOS values, host isolation, debugger refill effects, timed/untimed equality, and unchanged nominal costs.

The persistent-history regressions failed under the previous Thumb-continuation-only implementation.
Existing cold-entry tests now assert captured lane values instead of the removed unsupported-history diagnostic.
Region-crossing and strict unmapped-fetch diagnostics remain covered.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the prior shared-fetch reports exactly, including nominal cycle counts.
Debug and release reports also match for the same fixture paths.
These public suites remain regression checks, not independent IWRAM-history or hardware-timing conformance tests.
