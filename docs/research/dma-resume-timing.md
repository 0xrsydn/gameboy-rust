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
Pending resume changes the source code-access kind to non-sequential.
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
The later [ARM](arm-instruction-buffer.md) and [Thumb](thumb-instruction-buffer.md) instruction buffers retain instructions.
The [fetch-address timing extension](fetch-address-timing.md) now charges non-refill code at PC+4/PC+8 rather than the executing address.
A resumed access selects that fetch region's wait settings. It is still not a separately scheduled bus event.
DMA still executes only between instructions, not between data accesses or during CPU internal cycles.

The later [refill timing extension](refill-fetch-timing.md) replaces destination-only summaries with source-fetch and target-pair costs.
Resume changes the source access to N; the first target access independently remains N.
No extra access is appended. A resumed ROM branch can therefore cost more than its non-resumed refill.
The later [sequencing extension](fetch-access-sequencing.md) applies each instruction's data/internal effects to the following source fetch.
CPU history, DMA resume, and ROM page boundaries can all require N without duplicating an access.
A completed transfer after DMA can establish a new N requirement for the following fetch.
This baseline preceded active prefetch. The later [nominal queue](gamepak-prefetch.md) can satisfy an N resume request from queued code.

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
