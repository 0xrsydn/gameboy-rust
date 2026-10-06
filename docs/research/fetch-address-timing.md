# Non-refill timing at the actual fetch address

## Finding and evidence

The instruction buffer already sampled ARM P+8 or Thumb P+4 before executing the current instruction at P.
However, `Cpu::step_timed` charged non-refill code at P.
Near a memory-region, ROM wait-window, or 128 KiB page boundary, that selected the wrong bus cost.
It also charged a page-boundary access when execution later reached an already fetched instruction.

The [ARM7TDMI data-operation timing table](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/data-operations) identifies the normal instruction prefetch address as `pc+2L`.
Here L is the instruction length: four bytes in ARM state and two bytes in Thumb state.
An ordinary data operation overlaps this fetch; a register-controlled shift adds an internal cycle.
The [instruction-pipeline description](https://developer.arm.com/documentation/dvi0027/b/arm7tdmi/instruction-pipeline) provides the corresponding fetch/decode/execute model.

[NanoBoyAdvance at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) provides an implementation cross-check.
Its `Run` method takes the current retained opcode and fetches the next halfword/word at the advanced r15 before execution.
Its refill helpers advance visible r15 by two instruction widths after loading the target pair.
This supports using the new fetch address rather than the current retained opcode's address.

Existing [bus-cost rules](../hardware/cpu.md#instruction-and-bus-timing) supply the region, ROM wait-window, and forced page-boundary costs.
Existing [DMA resume evidence](dma-resume-timing.md) supports the retained one-shot sequence break.
This change uses those rules without claiming complete access-kind sequencing or refill timing.
No physical-hardware measurement or external-emulator differential run was performed.
All programs are original; no external source code or ROM was imported.

## Implemented subset

`Cpu::step_timed` captures the address of `Fetched.lookahead` before execution.
For a non-refill instruction, it passes that address to `bus_cycles` with the incoming instruction width.
The later [sequencing extension](fetch-access-sequencing.md) retains the incoming access kind from the preceding instruction.
The pending DMA resume still overrides that source access to N.
WAITCNT is captured before CPU execution, so a CPU store cannot retroactively change its own fetch cost.

The same rule applies to cold and retained pipelines.
Sampling alone adds no data-timing entry or device-clock update.
The returned code cost accounts for one new lookahead access, not an additional copy of it.
Cold current/decode fills still have no separate startup charge.
Successful untimed execution still consumes DMA resume without advancing clocks.
Failed execution retains the existing CPU/buffer, timing, and resume-state rollback rules.

A deferred lookahead error does not fail the current instruction early.
Its nominal code cost uses the attempted fetch address, even when supplied ROM bytes are missing there.
The strict instruction diagnostic occurs only when the missing slot becomes current.
An unmapped region retains the bus-cost helper's one-cycle fallback; this is diagnostic policy, not hardware abort timing.

## Examples with default WAITCNT

| Executing instruction | New fetch | Non-refill code cost |
| --- | --- | --- |
| ARM at `0x02fffff8` | IWRAM `0x03000000` word | 1 cycle, not EWRAM's 6 |
| Thumb at `0x02fffffc` | IWRAM `0x03000000` halfword | 1 cycle, not EWRAM's 3 |
| ARM at `0x07fffffc` | ROM `0x08000004` word | 6 cycles, not OAM's 1 |
| ARM at `0x0801fff8` | ROM page boundary `0x08020000` | 8 cycles for N+S |
| ARM at `0x08020000` | ROM `0x08020008` word | 6 cycles; no repeated page-boundary charge |

For ARM/Thumb ROM words/halfwords, a boundary cost now occurs two executed instruction positions earlier.
Crossing into another wait-state window selects that window's settings at the new fetch.
A completed DMA changes the access kind at this address; it does not append a second code access.

An ordinary ARM instruction starting directly at ROM base now costs 6 code cycles rather than 8.
The charged fetch is at ROM base plus eight, not at the page boundary.
This does not model the full hardware cost of initializing an empty pipeline.
Timer tests and the original timer demo now reflect this bounded rule.

## Deliberately retained limits

The later [refill timing extension](refill-fetch-timing.md) also charges source fetches and target pairs for branches and IRQ entry.

- Source S/N kinds now follow the preceding instruction's data/internal work.
  Data timing and device scheduling still do not form a complete chronological bus trace.
- Refills now charge one source fetch plus the target N+S pair, not an additional fetch beside the old destination-only summary.
- Thumb BL prefixes use their lookahead address; suffixes add the target pair.
- Cold fills have no separate startup charge, even though their samples update supported bus history.
- Devices advance once after the whole successful instruction. No per-access timer/IRQ update is introduced.
- Display contention, configurable EWRAM timing, and sub-instruction DMA arbitration remain unimplemented.
- The later [nominal prefetch queue](gamepak-prefetch.md) changes code costs when bit 14 is enabled.

This is a fetch-address correction within the existing timing model, not cycle-accurate emulation.
Game Pak prefetch followed this baseline; independent startup and invalidation timing validation remains necessary.

## Original regression coverage

New tests cover:

- ARM/Thumb memory-region crossings with cold and retained pipelines, compared with untimed execution.
- ROM 128 KiB boundaries using independent N/S tables for every wait setting and both prefetch-bit values.
- Wait-window crossings using the newly selected window's settings.
- DMA resume at a cross-region lookahead without duplicate code cost.
- CPU WAITCNT stores using old fetch settings and new settings on the next instruction.
- Deferred missing-lookahead diagnostics and unchanged nominal refill totals.
- Machine timer advancement from the corrected fetch cost before the next IRQ sample.

The address-sensitive tests failed with current-PC charging and pass with the new fetch address.
Existing ROM-start, boundary, BIOS-access, load-alias, and timer assertions were recalculated from PC+8/PC+4 costs.
Data results, access counts, flags, writeback, and failed-step preservation checks remain intact.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the prior fetch-boundary reports exactly, including cycle counts.
Debug and release reports also match for the same fixture paths.
The original timer demo's nominal total changes from 153 to 151 cycles because its initial ROM fetch is now charged at PC+8.
These are regression checks, not physical-hardware timing conformance results.
