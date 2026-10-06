# Persistent CPU fetch access kinds

S means sequential access; N means non-sequential access. W is the instruction width.

## Finding and evidence

Source-fetch addresses and refill target pairs were already timed separately.
However, the current instruction still selected its own source access kind.
Stores selected N immediately; loads, register shifts, and multiplies did not affect the following instruction.
IRQ entry always selected S unless DMA had completed.
This lost the sequence break between instruction execution and the next fetch.

The [ARM7TDMI load-multiple description](https://support.arm.com/documentation/ddi0210/c/Instruction-Cycle-Timings/Load-multiple-registers) separates the initial prefetch, data transfers, and final internal cycle.
Instruction cycle totals alone do not identify which neighboring fetch receives the bus-sequence change.
Two pinned implementations provide a source-level cross-check:

- [NanoBoyAdvance ARM handlers at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/handlers/handler32.inl) set the next pipeline access to N after transfers, swaps, register shifts, and multiplies.
- Its [Thumb handlers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/handlers/handler16.inl) do the same for corresponding Thumb operations.
- Its [CPU loop and refill helpers](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) consume that kind before execution, including discarded IRQ fetches.
  Failed ARM conditions set the following access to S. Target refills end with S.
- Its [bus timing](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/timing.cc) charges internal cycles and requested ROM wait costs separately when prefetch is inactive.
- [ares memory helpers at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/component/processor/arm7tdmi/memory.cpp) end a burst on internal cycles and data loads/stores.
  Its [instruction loop](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/component/processor/arm7tdmi/instruction.cpp) fetches before execution and restarts target refills with N followed by S.
  Its [ARM handlers](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/component/processor/arm7tdmi/instructions-arm.cpp) supply the corresponding internal cycles.

This is agreement between source implementations, not a physical-hardware timing result.
No external emulator was executed. No external implementation or ROM was imported into the repository.
Game Pak prefetch and within-instruction bus scheduling remain outside this subset.

## Implemented subset

The CPU instruction buffer retains the next source-fetch kind alongside its expected PC and instruction state.
The current source fetch consumes the incoming kind, with the existing one-shot DMA override.
Successful execution then selects the following kind:

| Completed operation | Following fetch |
| --- | --- |
| Arithmetic without an internal cycle, immediate shifts, status transfers, skipped conditions, BL prefix | S |
| Loads/stores, block/stack transfers, swaps | N |
| Register-controlled shifts and multiplies | N |
| Branch, PC load/write, SWI, or machine IRQ target refill | S |

Register-controlled shifts break the sequence even when the shift amount is zero.
Data operations break it even when data lies in RAM or I/O rather than ROM.
A PC load finishes its data/internal work before its target pair; the pair therefore establishes S for target execution.
The target pair itself remains N(T), S(T+W).

`Cpu::step` and `Cpu::step_timed` commit the same sequence state.
Clones and full CPU equality include it. Failed instructions preserve it with the existing instruction buffer.
A failed current-slot fetch cannot consume it. Deferred lookahead errors do not prevent a successful current instruction from updating it.
IRQ entry captures the incoming kind before exception entry discards the old buffer.
Masked IRQ requests leave it unchanged.

DMA history stays in `Memory`, separate from CPU continuation state.
Either source can require N; their combination does not add a second fetch or a fixed surcharge.
A completed load after DMA can establish a new N requirement for the following fetch.
Host reads/writes, clock advancement, and HALT/STOP idle do not consume the CPU continuation.

Cold startup still uses nominal S and has no separate current/decode fill charge.
Debugger invalidation and PC/state mismatches deliberately return to that cold policy.
They cannot reuse another continuation's access kind. Debugger invalidation does not clear Memory's DMA override.
This policy is not a claim about hardware reset/startup timing.
A status-restoration quirk that changes instruction state without a normal refill also retains the existing cold-buffer policy.

## Example with default WS0 settings

For ARM code away from ROM boundaries, S costs 6 cycles and N costs 8 cycles.
Consider a RAM word load followed by two ordinary arithmetic instructions:

1. The load consumes its incoming S source fetch, then performs its data read and internal cycle.
2. The first arithmetic instruction consumes N because of the preceding load.
3. The second arithmetic instruction consumes S again.

A store follows the same source-kind sequence, without the load's internal cycle.
The store no longer changes its own source fetch retroactively.
A WAITCNT store uses old settings for its source fetch, then new settings and N for the following fetch.
A branch after a load consumes N at the source and still charges the target N+S pair.

N is an access kind, not always a positive cycle penalty.
Some wait settings make N cheaper than S. Tests use independent N/S tables rather than adding a fixed cost.
ROM page boundaries can force N independently; a forced boundary and a retained N do not duplicate an access.

## Original regression coverage

Instruction-pair tests cover:

- Supported ARM/Thumb transfer families, block/stack operations, signed loads, swaps, shifts, multiplies, and status operations.
- Skipped conditions, zero shift amounts, BL prefixes, and ordinary arithmetic.
- Every ROM window and wait setting, both instruction widths, and stored prefetch-bit values.
- Repeated transfers, branches after loads, and PC-load refills that restore S.
- Timed/untimed equivalence, cloning, failed data accesses, missing current slots, and masked IRQ requests.
- DMA and CPU sequence-break composition, IRQ source timing, and handler refill state.
- Host and HALT isolation, debugger invalidation, PC mismatches, and page/wait-window crossings.

Existing rollback, instruction semantics, load data/internal costs, and deferred diagnostics remain unchanged.
WAITCNT and DMA tests now distinguish a consumed resume from a new sequence break caused by the completed instruction.

## Validation and report audit

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, clippy, rustdoc, Python preparation tests, native ROM windows, and all graphics smoke modes pass.
The release executable is native Mach-O arm64.
Public ARM, Thumb, memory, and BIOS checkpoint assertions pass; debug and release reports match.

Compared with the preceding refill-timing change:

- ARM nominal cycles increase by 208; Thumb increases by 106; memory increases by 32.
  Their other report fields remain unchanged.
- BIOS executes 963 fewer instructions and steps, with the same final nominal cycle total and all other report fields unchanged.

An original temporary trace runner compared both core versions against the pinned BIOS fixture.
The VBlank polling loop now costs 36 rather than 34 cycles because its load makes the following test instruction fetch N.
The updated core executes 321 fewer three-instruction polling loops.
IRQ entry moves to another position in the loop; the final checkpoint still has r12=0 and one IRQ entry.
This is an audit between our core versions, not external-emulator or hardware timing validation.

The original timer demo keeps its IRQ step and total duration.
Moving store fetch costs to following instructions shifts timer start two cycles earlier.
The timer therefore runs two additional cycles before the handler stops it.

## Remaining limits

- Cold-start costs and state-changing test/compare quirks are not hardware-validated.
- Data timing still uses an instruction-local access trace. This is not a complete bus-event scheduler.
- Device clocks still update once per instruction, IRQ entry, or DMA unit.
- Configurable EWRAM timing, display contention, and DMA during internal cycles remain unimplemented.
- IRQ synchronization delays and complete BIOS/general-bus latch history remain incomplete.

The later ordered timing transaction preserved these baseline totals.
The subsequent [nominal prefetch queue](gamepak-prefetch.md) uses those events and can accelerate code despite a CPU N request.
CPU access-kind rules remain separate from the queue's cartridge-bus timing.
The next timing work remains an active Game Pak queue, with verified startup and invalidation rules and independent tests.
