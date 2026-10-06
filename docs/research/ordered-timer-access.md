# Ordered timer data accesses

## Result

Machine timer reads and writes now use bus-access completion times instead of the instruction's initial clock.
The unchanged [prefetch cancellation probe](prefetch-cancellation.md) matches both published interval totals and unadjusted timer readings.
Its program, published observations, and measurement code were not changed to obtain this result.

This is a timer-specific extension to nominal timing, not a complete cycle-accurate device scheduler.
Display events, DMA arbitration, timer startup delays, and IRQ synchronization remain separate limits.
No physical-hardware measurement or external-emulator differential run was performed locally.

## Evidence and phase choice

The preceding probe isolated the error: its missing interval equaled the difference between the final source-fetch costs.
The opcode queue computed those costs correctly, but the timer read occurred before they advanced device time.
Moving only the probe's reported result would conceal that error rather than correct device behavior.

The reviewed [Jgenesis bus at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/bus.rs) charges the I/O cycle before register access.
Its timer reads and writes receive the resulting clock value.
This supports the implemented completion-phase rule for a one-cycle I/O access.
No source code was imported from that implementation.

The published subtraction experiment validates relative read intervals.
It does not independently establish every absolute timer edge, startup delay, or control-write phase.
Those remain explicit validation limits.

## Staged clock ownership

`Memory` owns an optional `TimerStep` during each machine instruction, IRQ entry, or DMA unit.
The transaction copies the four timers, shared divider phase, and timer IF bits, not RAM or the entire I/O object.
The later [shared-prescaler correction](timer-prescaler.md) replaced private timer remainders with this common divider.
Its elapsed timestamp starts at zero and increases monotonically.
It has no heap allocation or event log.

For a CPU instruction:

1. Stage timer progress through the source fetch.
2. For each data access, stage its elapsed cycles before reading or writing timer registers.
3. Stage internal cycles and any target-pair fetches in their existing order.
4. On success, commit timers and timer IF bits at the final timestamp.
5. Advance the other devices and the machine clock once, without ticking timers again.

One word access has one sampling time for all four byte lanes.
Reload bytes merge before control bytes at that same time.
Block transfers have separate completion times for successive words.
A start write cannot count earlier source cycles. A stop write includes clocks through its completion.
Shared divider edges, cascaded overflow pulses, and reload behavior use the timer-bank implementation.

Timer IF reads include requests raised through the access phase.
Acknowledgement clears timer requests already present at that phase; later work can raise them again.
Committing timer flags preserves non-timer IF bits.
Those other sources still follow the existing bulk event model, so IF is not a fully cycle-accurate cross-device snapshot.
IRQ delivery remains at machine-step boundaries, never halfway through an instruction.
A committed enabled timer request wakes HALT. STOP does not use timer requests as a wake source.

### DMA

Each unit uses the existing nominal two-cycle startup on its first transfer.
Timer sampling follows startup plus source cost; destination writes follow the destination cost.
A failed source or destination discards the timer transaction.
A successful unit commits timer state before other device clocks and DMA completion IRQ handling.
This extends the existing whole-unit scheduler; it does not establish physical DMA startup or preemption timing.

### Errors and API isolation

CPU diagnostics discard staged time, counter values, shared divider progress, and pending timer requests.
This includes failures before instruction fetch and failures after earlier block-load accesses.
Existing store validation still prevents partial writes when a block transfer has an invalid destination.
Previously completed machine steps remain committed.

`Cpu::step` and `Cpu::step_timed` remain CPU-only APIs.
They update CPU state, ordinary register writes, instruction retention, and enabled opcode-prefetch timing as before.
They do not create a timer transaction or advance device clocks.
Host reads and writes likewise remain clock-free.

`Memory::advance_cycles` and HALT idle intervals use the existing bulk timer advancement.
STOP idle freezes all clocks. The instruction entering STOP still pays its complete nominal duration.
Instruction fetches from I/O are not given the data-access timer sampling rule.
Other device reads, writes, rendering, and event scheduling retain their documented limits.

## Validation

Original tests cover ARM/Thumb loads, coherent byte lanes, start/stop stores, and ordered block transfers.
They check old-reload overflow behavior, prescaler retention, cascades, timer IF reads, acknowledgement, and later reassertion.
Other tests cover non-timer IF preservation, deferred IRQ delivery, DMA phases, rollback, and CPU-only isolation.
Capture-enabled and capture-disabled steps check that display clock splits do not tick timers twice.
BIOS HALT/STOP stores check final-cycle ownership and timer wake behavior.

The cancellation example exits successfully in debug and release on Darwin arm64.
Its CSV reports match between build profiles. No correction is applied to loaded timer values.
Workspace/core/demo tests, formatting, clippy, rustdoc, Python tests, native ROM windows, and graphics smoke modes pass.
Pinned public ARM, Thumb, memory, and BIOS reports remain byte-identical to the preceding change.

The original timer demo retains its cycle count and IRQ-entry step.
Its stopped counter changes from `0xfffb` to `0xfff4`: start excludes nine preceding cycles, while stop includes two bus cycles.
The alias-load regression now samples `0x00c0fffb` after source and data cycles, instead of the previous `0x00c0fffa`.
These expectation changes follow the access phases; the independent cancellation program and observations remain unchanged.

Next, validate absolute timer startup/control edges and independently test full-buffer prefetch restart and page boundaries.
Extend other device observations only with explicit clock ownership and rollback coverage.
