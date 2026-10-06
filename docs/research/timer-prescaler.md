# Shared timer prescaler phase

## Result and scope

All four timers now derive independent-clock ticks from one free-running system-clock divider.
Enabling, disabling, restarting, or changing a timer's clock source does not reset that divider.
This replaces the previous per-timer phase, which restarted on enable or clock-source changes.

The correction covers divider ownership and edge selection in the current immediate-write timer model.
It does **not** add hardware startup delays, delayed register writes, or IRQ synchronization delays.
Those behaviors need separate edge tests; adding a fixed startup delay alone would be incomplete.
No physical-hardware measurement or external-emulator differential run was performed locally.

## Evidence reviewed

- [Jgenesis timers at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/timers.rs) derive ticks from absolute clock values shifted by the selected divisor.
  Pending register writes and newly enabled reloads have separate handling.
- [NanoBoyAdvance timers at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/timer/timer.cc) derive prescaler alignment from the scheduler timestamp.
  Its [channel definitions](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/hw/timer/timer.hh) separate pending writes from active timer control.
  The implementation also handles a possible overflow from an old `0xffff` counter during enable/reload timing.

Both implementations use a common clock origin rather than a private period restarted for each timer.
Their startup handling also shows why divider phase and enable delay must be treated separately.
The current correction uses the shared-clock rule without claiming their complete transition behavior.
No implementation code was imported.

Search also identified a [VBA-M prescaler change](https://github.com/visualboyadvance-m/visualboyadvance-m/commit/92154dcd2ef7fca8c8afa8fced2f350ad2d2452a)
and the [mGBA timer suite](https://github.com/mgba-emu/suite/blob/04ada216ee13c56d786e54636ac980a71d791145/src/timers.c).
These remain follow-up leads, not local test passes or implementation oracles.

## Implemented divider

Synthetic reset starts the shared phase at zero.
Only the low ten system-clock bits are needed for divisors 1, 64, 256, and 1024.
The phase advances with emulated time even when all timers are disabled or cascaded.
It does not depend on host wall time or display-frame position.

For an interval from clock `start` to clock `end`, an independently clocked timer receives:

```text
ticks = floor(end / divisor) - floor(start / divisor)
```

This counts divider edges after the starting timestamp and through the ending timestamp.
An enable at an already completed edge does not replay that edge.
A clock-source change selects another output of the same divider, not a new clock origin.
Leaving cascade mode resumes from that existing divider position.
Cascade mode still counts predecessor overflows and ignores the prescaler selection field.

The core counts ticks and overflow pulses in bulk without iterating once per cycle.
Wide arithmetic handles maximum `u32` clock batches before retaining the low divider bits.
Overflow scheduling subtracts the current shared remainder, so HALT can stop at the next actual divider edge.

For example, under the current immediate-enable policy, a /64 timer enabled at clock 63 ticks at clock 64.
The old private-phase model incorrectly waited until clock 127.
This example defines the implemented no-delay subset, not a claim about an enable write adjacent to a hardware startup edge.

## Clock ownership and rollback

The divider belongs to the timer bank, not an individual timer.
Each machine `TimerStep` copies the shared phase with the counters and timer IF bits.
CPU and DMA bus phases advance that copy. Success commits it; a failed step discards it.
Other devices' bulk updates do not advance the committed divider again.

Host clock advances and HALT idle periods advance the same divider.
STOP freezes it, and keypad wake resumes the retained phase.
Host register access and CPU-only stepping do not advance time or reset the divider.
See [ordered timer accesses](ordered-timer-access.md) for transaction and API ownership.

## Original regression coverage

Tests exercise every low-ten-bit phase and all divider selections, including zero-cycle advances and completed-edge enables.
Staggered timer starts verify shared edges. Clock-source changes, restarts, and cascade transitions preserve the common origin.
A literal cycle-by-cycle reference checks randomized control, reload, acknowledgement, and clock sequences.
It uses absolute edge tests, not the implementation's phase accumulator or bulk overflow division.
Maximum-size batches check bounded progress and divider wrap.

Machine tests cross a shared edge during CPU stores, timer loads, and DMA source/destination phases.
Failed CPU and DMA steps must not consume that edge or retain its IRQ request.
HALT after a machine timer start must use the committed phase for its next-event bound.
STOP/keypad tests verify that stopped time does not move the divider.

## Validation and remaining work

Workspace/core/demo tests pass in debug and release on Darwin arm64.
Formatting, clippy, rustdoc, Python tests, native ROM windows, and all graphics smoke modes pass.
The release executable remains native Mach-O arm64.
Public ARM, Thumb, memory, and BIOS reports remain byte-identical to the preceding ordered-timer change.
The timer demo trace and both profiles of the cancellation probe are unchanged.
The probe still matches all its published observations, but it uses divisor one and does not validate prescaler startup edges.

Next, use independent timer-edge observations to define delayed reload/control writes and enable behavior.
Include overflow from the previous counter, cascade transitions, same-cycle reads, and IRQ timing before adding startup delays.
Full-buffer prefetch restart and page-boundary validation remain separate work.
