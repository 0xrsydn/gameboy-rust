# Source-fetch and target-pair refill timing

## Finding and evidence

The instruction buffer already sampled a branch's old-state lookahead and its two target instructions.
However, timing charged three destination accesses: N(T), S(T+W), and S(T+2W).
That omitted the source-region cost and charged the third target fetch before target execution.
The totals often matched within one memory region, hiding cross-region and instruction-state errors.
Machine IRQ entry also omitted the discarded source sample entirely.

The [ARM7TDMI branch timing description](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/branch-and-branch-with-link) specifies three accesses:

1. Prefetch from the current PC while calculating the destination. The decision cannot prevent this fetch.
2. Fetch the branch destination.
3. Fetch destination plus one instruction length to refill the pipeline.

The [software-interrupt and exception-entry description](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/software-interrupt-and-exception-entry) likewise describes a forced-address construction cycle followed by pipeline refill.
Its first address is `pc+2L`, using the incoming instruction state.
The vector pair uses ARM state.

[NanoBoyAdvance at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) provides a source cross-check:

- `Run` fetches the old-state lookahead before instruction execution.
- `ReloadPipeline16` and `ReloadPipeline32` read the target N and the next instruction S.
- `SignalIRQ` first performs a discarded halfword/word fetch using the incoming state and access kind.
  It then changes mode/state and reloads the ARM vector pair.

This supports source-address and target-pair accounting, not complete chronological bus sequencing in this emulator.
The existing [DMA resume evidence](dma-resume-timing.md) supports changing the next source access to N.
No physical-hardware measurement or external-emulator differential run was performed.
Tests and tracing code are original. No external source code, ROM, or BIOS was added to the repository.

## Implemented subset

For an instruction executing at P, `Cpu::step_timed` charges its sampled lookahead once:

- ARM source: P+8, word width.
- Thumb source: P+4, halfword width.
- Access kind: the existing instruction-local summary, overridden to N after successful DMA.
- WAITCNT: captured before instruction effects.

If the instruction refills, timing also charges N(T) and S(T+W) using the resulting state.
The existing pipeline samples supply those instructions and update supported local history.
T+2W is sampled and charged when the first target instruction executes, not during the refill.
The same rule covers taken branches, BX, PC loads/writes, SWI, saved-state returns, and Thumb BL suffixes.
Untaken conditions and BL prefixes do not add a target pair.

Accepted machine IRQ entry samples the discarded incoming-state P+2W fetch, then the ARM vector pair.
It does not fetch or execute the interrupted current opcode, nor invent a cold current/decode fill.
The discarded sample drives its addressed IWRAM lanes when mapped; other regions leave that local history unchanged.
An unavailable discarded sample cannot prevent entry and is not retained as a current-slot diagnostic.
Unavailable vector samples fail only when execution reaches them.
IRQ sampling does not commit a BIOS instruction snapshot because no BIOS instruction executed.
The separate successful-BIOS-instruction history policy remains unchanged.

Masked IRQ requests have no fetch, history, CPU, or resume-state effects.
DMA still takes priority at machine-step boundaries.
Successful IRQ entry consumes pending DMA resume through normal machine-step completion.
Devices advance once by the total entry cost, not between its accesses.
The public memory-free `Cpu::enter_exception` and `Cpu::take_interrupt` APIs remain unchanged.

## Examples with default WAITCNT

| Operation | Source | Target pair | Code cycles |
| --- | --- | --- | --- |
| IWRAM ARM BX to Thumb WS1 | 1 | 5 + 5 | 11 |
| IWRAM Thumb BX to ARM WS0 | 1 | 8 + 6 | 15 |
| ARM WS0 branch to IWRAM | 6 | 1 + 1 | 8 |
| ARM WS0 branch within WS0 | 6 | 8 + 6 | 20 |
| Same branch after DMA | 8 | 8 + 6 | 22 |
| IRQ from ARM WS0 | 6 | 1 + 1 | 8 |
| IRQ from ARM WS0 after DMA | 8 | 1 + 1 | 10 |

These examples exclude ROM page boundaries.
Every access independently selects its actual region, wait window, and forced-N 128 KiB boundary cost.
A target pair can cross a region or wait-window boundary.
A boundary at T+2W is charged on target execution, not during the preceding branch.
DMA can produce two N accesses: the source fetch and the first target fetch. No fourth fetch is added.

## Original regression coverage

Tests cover:

- Incoming/outgoing ARM and Thumb widths, all ROM wait settings, and the stored prefetch bit.
- Boundaries in the first, second, and third target slots, plus split wait-window target pairs.
- ROM source boundaries and DMA resume before a RAM refill, with no delayed resume penalty.
- Cross-region source lookahead, data/internal costs for PC loads, and saved-state returns.
- IRQ source widths, missing ROM, masked requests, LR/SPSR preservation, and device-clock totals.
- IRQ IWRAM word and halfword lane updates, cold entry without invented fills, and retained Thumb entry.
- Missing-vector rollback after accepted entry, BIOS-history isolation, and timed/untimed CPU equivalence.

## Validation and report changes

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, clippy, rustdoc, Python preparation tests, native ROM windows, and all graphics smoke modes pass.
The release executable is native Mach-O arm64.
Public ARM, Thumb, memory, and BIOS checkpoint assertions pass. Debug and release reports match.

Compared with the preceding fetch-address change, the ARM, Thumb, and memory reports each lose five nominal cycles.
Their step counts and all other report fields remain unchanged.
The BIOS report gains three instructions and 29 nominal cycles; all other fields remain unchanged.

A temporary original trace runner compared the preceding and updated cores against the pinned BIOS fixture.
The changed VBlank arrival position causes one extra three-instruction polling loop, adding 34 cycles.
Cross-region firmware transfers and IRQ entry together remove five cycles, giving the observed net increase of 29.
The trace still reaches the same checkpoint with r12=0 and one IRQ entry.
This trace comparison is a regression audit between our core versions, not external hardware validation.

The original timer demo retains its total nominal duration and IRQ step.
IRQ entry gains five cycles before the handler stops the timer; the return loses five cycles afterward.
The stopped counter therefore increases by five even though the final machine-cycle total is unchanged.

## Remaining limits

- Source access kinds still use instruction-local summaries, including the existing store rule.
  Neighboring instruction/data-access sequencing is not a complete chronological bus trace.
- Cold current/decode fills have no separate startup timing charge.
- BIOS retained history is not a complete refill/data-access bus latch.
- Missing instruction bytes remain diagnostics, not hardware prefetch aborts.
- Game Pak prefetch, per-access device updates, display contention, configurable EWRAM timing, and sub-instruction DMA arbitration remain unimplemented.

The next timing work is independently verified access-kind sequencing between instructions and data accesses.
