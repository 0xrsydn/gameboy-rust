# Game Pak prefetch: nominal queue and evidence

## Current result

WAITCNT bit 14 enables a nominal Game Pak opcode prefetch queue.
The queue tracks eight halfwords, partial transfers, full-buffer stops, and cancellation stalls.
It follows ordered CPU timing events and the existing instruction-boundary DMA scheduler.
This is source-backed timing behavior, not hardware-verified accuracy.

The queue stores addresses and timing metadata, not ROM bytes.
CPU instruction retention, BIOS protected-read history, and IWRAM local lanes remain separate.
Background progress cannot cause an early ROM-file lookup or change instruction values.

## Sources reviewed

- [GBATEK GamePak Prefetch](https://www.problemkaputt.de/gbatek-gba-gamepak-prefetch.htm) describes eight 16-bit entries, opcode-only service, and idle-bus progress.
- [mGBA's cycle-counting article](https://mgba.io/2015/06/27/cycle-counting-prefetch/) explains independent cartridge progress and partially completed fetches.
  Its examples and linked timing suite provide validation leads, not a local hardware-test pass.
- [NanoBoyAdvance bus timing at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/timing.cc) distinguishes hits, partial fetches, misses, and cancellation stalls.
- [ares prefetch at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/prefetch.cpp) uses halfword entries and stops at capacity or page boundaries.
  Its [bus dispatcher](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) separates opcode requests, data requests, and other-memory progress.
- [Jgenesis prefetch at fab6e2cc](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/prefetch.rs) independently supports halfword entries, full-buffer stopping, and last-cycle cancellation stalls.
  Its [bus integration](https://github.com/jsgroth/jgenesis/blob/fab6e2ccc60e492dd68b7f1e927b0829a6d80195/backend/gba-core/src/bus.rs) separates CPU delivery cycles from background transfers and DMA ownership.
- [NanoBoyAdvance 128 KiB boundary test at cc3f4a28](https://codeberg.org/nba-emu/hw-test/src/commit/cc3f4a286cdef823980d9b353bd70befc9927d28/bus/128kb-boundary/source/main.c) tests data LDM and DMA boundaries.
  Reading this test does not validate active opcode-queue crossings. It was not run here.
- [PrefetchAbuse at 9ca57c13](https://github.com/zaydlang/PrefetchAbuse/blob/9ca57c13da7e3c569937f99a42e7c1caca029a2d/src/main.c) supplies published hardware read-cancellation observations.
  A later [original probe](prefetch-cancellation.md) matches the instruction-boundary differences, but exposes incorrect bulk timer sampling.
  The source, README, and build assumptions were reviewed. No upstream implementation or ROM was imported.

No external implementation or ROM was added to the repository.
No physical-hardware measurement or external-emulator differential run was performed.

## Implemented queue rules

S means sequential access; N means non-sequential access.
Raw ROM costs include one clock plus the configured wait states for each halfword.
The public `bus_cycles` helper remains stateless and does not apply prefetch.

1. A ROM code miss pays the requested raw cost, plus any cancellation stall.
   It starts background fetching at the following halfword address.
2. Internal cycles and non-cartridge memory accesses advance an already active stream.
   They cannot start a stream by themselves.
3. A code request must match the queue head. CPU N requests can still match queued code.
   A queued Thumb halfword or complete ARM word costs one CPU-side cycle.
4. That CPU-side cycle also advances background work when production is active.
   A partial ARM word waits separately for its missing halfwords.
5. Background production stops when an advance observes all eight entries occupied.
   It remains stopped while the queue drains. The next empty demand restarts with N timing.
6. ROM data accesses cancel the stream instead of consuming opcode entries.
   A nonmatching ROM code request also cancels it before starting a new stream.
7. Cancellation costs one additional cycle when an active halfword has one cycle remaining.
   A paused or fully occupied queue does not add that stall.
8. Background production pauses before a 128 KiB ROM boundary.
   Demand at that boundary uses raw forced-N timing and starts the next stream.
   No background transfer crosses beyond the final ROM window.

The full-buffer policy follows ares and Jgenesis.
The page-boundary policy follows ares conservatively; the reviewed sources differ in this area.
The cancellation cycle is included in the requesting code or data field of `StepTiming`.
These rules have original regression tests, not a hardware conformance result.

## Timing transactions and ownership

`Memory` owns the committed queue because CPU, DMA, and HALT share cartridge-bus time.
Each `CpuTiming` transaction stages a copy and applies events in order:

1. Source instruction fetch at ARM P+8 or Thumb P+4.
2. Actual data accesses, using aligned bus addresses and widths.
3. Internal cycles from the incoming instruction classification.
4. Refill target N and target+width S accesses, using the resulting instruction state.

IRQ entry records the discarded incoming-state source, then the ARM vector pair.
Non-ROM code fetches advance the existing ROM stream without consuming it.
A branch can therefore preserve the stream while executing a BIOS handler.
A later ROM fetch still needs an exact head match to use queued timing.

Success commits queue progress. A diagnostic discards all staged progress, including earlier successful data accesses.
Missing lookahead or target bytes retain deferred diagnostics and nominal attempted-fetch costs.
Device clocks still advance once after a successful machine step, not after each timing event.

`Cpu::step_timed` updates the queue and returns costs without advancing devices.
`Cpu::step` uses the same path when prefetch is enabled, but discards returned costs.
With prefetch disabled, the CPU-only path retains its previous untimed behavior.
Host reads, setup writes, and `Memory::advance_cycles` do not supply queue progress.
Host WAITCNT writes do apply queue configuration.

Cold entry, debugger invalidation, and PC/state mismatches clear staged queue metadata.
They preserve the existing nominal S startup policy and add no current/decode fill charge.
Accepted IRQ entry uses the interrupted pipeline's cold/retained state before replacing that pipeline.
Failed CPU steps preserve the previously committed queue.

### WAITCNT changes

A store pays source and data costs using the previous settings.
After the write, changing ROM wait fields or bit 14 clears timing metadata immediately.
Changing only SRAM or PHI fields preserves the stream.
CPU, host byte/halfword/word, and DMA writes use this configuration policy.

This reset policy is deliberate, not a verified model of live hardware reconfiguration.
Jgenesis models retained entries after disable; that behavior is not implemented here.
Exact enable/disable transitions and changes during a partial transfer need independent tests.

### DMA and idle

A DMA unit stages queue progress independently of the CPU transaction.
Its existing two startup cycles precede source and destination accesses in the nominal model.
RAM-only DMA supplies idle cartridge time. Cartridge DMA cancels the stream and can incur the completion-edge stall.
A failed unit preserves queue state and clocks. A successful WAITCNT destination applies the new configuration.
DMA's CPU-resume N request remains separate; a matching queue entry can still satisfy that request.

`HaltIdle` advances an active queue with its idle cycles. `StopIdle` freezes it.
Machine device updates do not advance the queue a second time.
Exact DMA startup/completion placement, bus arbitration, and hardware HALT/STOP edges remain unverified.

## Original regression coverage

Queue tests cover all ROM windows, all N/S settings, ARM/Thumb widths, partial words, capacity, and restart.
They check every cancellation phase around a fill, exact-head matching, data cancellation, and page/window boundaries.
Configuration tests distinguish relevant fields from PHI/SRAM fields.
A seeded batched-versus-single-clock comparison checks progression consistency, not independent hardware correctness.

Integration tests cover RAM loads, register shifts, ROM data stalls, branch hits/misses, and PC-load refills.
They check timed/CPU-only equivalence, debugger invalidation, IRQ entry, WAITCNT writes, DMA, HALT, and STOP.
Failure tests cover late block loads, missing ROM bytes, failed DMA, retries, and host isolation.
Instruction-pair matrices separately assert CPU N/S kinds and prefetch-adjusted costs.
Existing ordered-event tests retain source/data/internal/refill ordering checks, including a full-register LDM with PC.
Test builds retain an event log; production builds allocate no event log or queue storage on the heap.

## Validation and remaining limits

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, clippy, rustdoc, Python preparation tests, native ROM windows, and all graphics smoke modes pass.
The release executable is native Mach-O arm64.
Public ARM, Thumb, memory, and BIOS reports match the preceding ordered-timing change exactly, including cycles and step counts.
Debug and release reports also match. Original CPU and timer demo traces are unchanged.
These public checkpoints do not independently validate active prefetch timing.

The later [cancellation comparison](prefetch-cancellation.md) matches published read observations only at instruction boundaries.
Actual timer readings still fail because device updates occur after the timer-load instruction.
Next, implement ordered timer observations and validate full-buffer restart and page boundaries independently.
Cold-start timing, live WAITCNT reconfiguration, per-access device scheduling, and sub-instruction DMA arbitration remain incomplete.
Exact bus history, display contention, configurable EWRAM timing, and timer/IRQ delays remain separate work.
