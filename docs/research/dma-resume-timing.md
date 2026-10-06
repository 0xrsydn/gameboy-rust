# Non-sequential CPU resume after DMA

## Finding and evidence

DMA can break the CPU's sequential bus access even when the transfer uses only work RAM.
The prior instruction timing path always used its instruction summary, without retaining DMA ownership changes.
An ordinary ROM instruction after DMA therefore still received a sequential code-access cost.

Sources reviewed:

- [NanoBoyAdvance force-nseq-access test at cc3f4a28](https://codeberg.org/nba-emu/hw-test/src/commit/cc3f4a286cdef823980d9b353bd70befc9927d28/dma/force-nseq-access/source/main.c) compares EWRAM-source and ROM-source transfers during a short instruction sequence.
  Both tests specify the same timer result. This is a whole-sequence expectation, not an isolated first-fetch measurement.
- [NanoBoyAdvance bus at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/bus.cc) forces non-sequential ROM access when the previous access was DMA and the current access is CPU.
  The condition does not require the DMA source or destination to be ROM.
- [ares DMA at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/dma.cpp) marks ARM7 accesses non-sequential when DMA takes bus ownership.
  Its [bus dispatch](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) uses that state for CPU burst selection.

These sources support a sequence break. They do not establish exact timing for our instruction-boundary scheduler.
The public test source was reviewed outside the repository. Its ROM was not imported, built, or executed here.
No physical-hardware measurement or external-emulator differential run was performed.

## Bounded implementation

A successful DMA unit sets a pending non-sequential CPU resume in `Memory`.
Further units and channel changes keep that single pending state. They do not queue additional costs.
A failed unit cannot create or consume it. A blocked-source unit that successfully writes known channel data sets it normally.
Register configuration, host reads/writes, and clock-only advancement do not consume it.

Before instruction execution, `Cpu::step_timed` captures the pending state and WAITCNT.
For a non-refill instruction, pending resume changes the nominal code-access kind to non-sequential.
The existing bus-cost function applies the code width and ROM-window wait settings.
An ARM word still uses a non-sequential first halfword and a sequential second halfword.
This is a change of access kind, not a fixed positive surcharge: some wait settings make N cheaper than S.

Successful CPU execution consumes the pending state in timed and untimed APIs.
Skipped conditions consume it too. Failed fetches, unsupported instructions, and failed data accesses preserve it for retry.
Execution in internal memory consumes it even when N and S costs are equal.
The state does not wait for a later ROM instruction.

Machine IRQ entry consumes it through successful CPU-step completion.
HALT/STOP idle does not consume it. Normal device progress, wake-up, and IRQ sampling rules still apply.
A CPU WAITCNT store uses the pre-instruction settings for its code access.
A completed DMA WAITCNT store changes the settings before CPU resume.

## Limits

The timing model has no per-access fetch scheduling or DMA arbitration.
The later [ARM](arm-instruction-buffer.md) and [Thumb](thumb-instruction-buffer.md) instruction buffers retain instructions without changing these nominal costs.
It charges the current instruction address, not a separately scheduled PC+4/PC+8 pipeline fetch.
DMA still executes only between instructions, not between data accesses or during CPU internal cycles.

Existing refill summaries remain destination-based `1N+2S` costs.
A resumed branch or exception consumes pending state without adding another nominal access.
This prevents double-counting the existing N component, but does not model the discarded old-PC fetch separately.
Stores and 128 KiB ROM boundaries already use N and receive no duplicate cost.
Game Pak prefetch remains unimplemented; WAITCNT bit 14 does not alter this nominal timing path.

This change does not select a DMA value for CPU open-bus reads.
DMA channel data, persistent local IWRAM lanes, and protected BIOS history remain separate.
General bus-data ownership and exact resume timing still need independent per-access validation.

## Original regression coverage

Tests cover ARM/Thumb widths, all three ROM windows, every N/S wait setting, and stored prefetch-bit values.
They check RAM/ROM DMA sources, repeated units, channel preemption, blocked-source completion, and configuration-only isolation.
Other checks cover CPU/DMA errors, mixed timed/untimed APIs, host isolation, WAITCNT writes, skipped conditions, stores, and ROM boundaries.
Refills, IRQ entry, internal-memory execution, HALT/STOP idle, timer advancement, and subsequent IRQ delivery have explicit checks.
The new resume cases failed on the prior sequential-only path before implementation.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match their previous passing results exactly.
These suites are regression checks, not independent conformance tests for DMA resume timing.
