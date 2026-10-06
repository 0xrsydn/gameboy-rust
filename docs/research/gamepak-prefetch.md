# Game Pak prefetch: evidence and timing prerequisites

## Current result

Game Pak prefetch is **not enabled in the core yet**.
WAITCNT bit 14 remains stored without accelerating instruction fetches.
This change prepares ordered CPU timing transactions without changing cycle totals or instruction results.

The previous timing path collected data costs during execution, then calculated source and refill costs afterward.
That produced correct existing totals but could not directly advance a prefetch queue in bus-access order.
A prefetch implementation needs to distinguish cartridge-bus occupation from time available for background fetching.
An aggregate instruction cost cannot identify when to fill or invalidate a queue.

## Sources reviewed

- [GBATEK GamePak Prefetch](https://www.problemkaputt.de/gbatek-gba-gamepak-prefetch.htm) describes eight 16-bit entries, opcode-only service, and prefetch during internal or other-memory accesses.
- [mGBA's cycle-counting article](https://mgba.io/2015/06/27/cycle-counting-prefetch/) explains independent cartridge-bus progress, partially completed fetches, and instruction-pair examples.
  Its examples and linked timing suite provide leads for independent validation, not a local hardware-test pass.
- [NanoBoyAdvance bus timing at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/timing.cc) distinguishes queue hits, in-progress fetches, misses, and cancellation stalls.
- [ares prefetch at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/prefetch.cpp) provides an independent halfword-queue implementation.
  Its [bus dispatcher](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/bus.cpp) separates opcode requests, data requests, and other-region progress.

No external implementation or ROM was added to the repository.
No physical-hardware measurement or external-emulator differential run was performed.

## Agreement and unresolved details

The sources agree on the main model:

- Prefetch is controlled by WAITCNT bit 14.
- The buffer serves Game Pak opcodes, not arbitrary ROM data loads.
- Internal CPU work and accesses to other memory can give the cartridge bus time to fetch ahead.
- A matching ready entry avoids ordinary ROM waits.
- A request for an in-progress fetch waits for the remaining work rather than restarting the full access.
- Cartridge data traffic and nonmatching code requests interact with the active prefetch sequence.

However, the implementation details are not interchangeable:

| Area | Source observation | Required follow-up |
| --- | --- | --- |
| Entry granularity | NanoBoyAdvance groups ARM words; ares tracks halfwords | Verify partial ARM fetches and ARM/Thumb transitions |
| Full buffer | ares stops production and restarts after draining; NanoBoyAdvance uses an instruction-sized countdown/count model | Establish capacity, pause, drain, and restart timing independently |
| ROM page boundary | ares prevents background reads across the forced-N boundary; NanoBoyAdvance's queue step uses its retained duty | Verify 128 KiB boundaries without assuming a normal ready-entry hit |
| Cancellation | Both contain completion-edge stall handling | Verify the exact halfword phase and which code/data requests trigger a stall |
| DMA and idle | Prefetch shares cartridge time with other bus owners | Verify RAM-only DMA, cartridge DMA, HALT/STOP, and resume separately |
| WAITCNT changes | Queue state and current transfer timing depend on register changes | Define enable, disable, and changed-wait behavior before integration |

These differences do not establish that either implementation is wrong.
They show why copying one queue policy is not sufficient evidence for the entire subsystem.
The first implementation should state its supported cases and retain explicit limits for unresolved behavior.

## Ordered CPU timing transactions

`CpuTiming` replaces the data-only accumulator.
A timed CPU instruction now records these events in order:

1. Source instruction fetch, using the incoming access kind and instruction width.
2. Actual data reads/writes, using their aligned bus addresses and widths.
3. Internal CPU cycles from the incoming instruction classification.
4. Refill target N and target+width S accesses, using the resulting instruction state.

S means sequential access; N means non-sequential access.
An instruction without data, internal work, or a refill omits those events.
The two refill events occur in `Cpu::refill_pipeline`, alongside its existing mapped target samples.
No separate refill-cost formula runs after execution.
IRQ entry records its discarded incoming-state source fetch before its ARM vector pair, without a data or internal event.

Code timing retains the entry WAITCNT snapshot.
Data timing retains the settings observed at each actual bus access, including pre-write settings for a WAITCNT store.
The next instruction uses the new WAITCNT settings.
The existing data N/S rule and persistent CPU source-fetch kind are unchanged.

The transaction returns the same `StepTiming` fields as before.
A diagnostic discards the entire transaction, including any data events recorded before a late failure.
Missing lookahead or target bytes retain their existing deferred diagnostic policy and nominal attempted-fetch cost.
Device clocks still advance once after a successful machine step.
This is ordered cost accounting, not a cycle-by-cycle bus scheduler.

Untimed execution, host inspection, cold-fill sampling, and DMA do not join a CPU timing transaction.
They retain their existing behavior and do not create extra code/data charges.
Untimed CPU execution still updates instruction retention and the next fetch kind, but does not compute a timing transaction.
A future active prefetch model must define timing-state progression for this API explicitly.

## Original regression coverage

Test builds retain events from the last successful CPU timing transaction.
Production builds retain only totals, the code WAITCNT snapshot, and data-sequence state; they allocate no event log.
The test log covers the maximum supported block load, including PC, without truncation.
Tests inspect event order and metadata independently of total cycle assertions.

Coverage includes:

- Word PC loads and the full register-list LDM sequence.
- Thumb POP with word data and halfword target fetches.
- ARM-to-Thumb BX, taken branches to fallthrough, and skipped conditions.
- Swaps, register-shift internal cycles, and absence of duplicate source/refill events.
- WAITCNT stores with old source settings and new settings on the next fetch.
- IRQ entry after a Thumb load, including the retained non-sequential source kind.
- Failed data accesses, late block-load failure, missing current/lookahead slots, and retry isolation.
- Host reads, untimed CPU execution, and DMA isolation from the CPU trace.

## Validation

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, clippy, rustdoc, Python preparation tests, native ROM windows, and all graphics smoke modes pass.
The release executable is native Mach-O arm64.
Public ARM, Thumb, memory, and BIOS reports match the preceding sequencing change exactly, including cycles and step counts.
Debug and release reports also match. Original CPU and timer demo traces are unchanged.

## Next implementation work

Implement and test queue progress against these ordered timing events before enabling WAITCNT acceleration.
Keep the cartridge queue separate from CPU instruction retention, BIOS protected-read history, and IWRAM local lanes.
Resolve startup, capacity, page boundaries, partial ARM fetches, cancellation, and WAITCNT changes with independent expectations.
DMA/idle progression and diagnostic rollback need separate tests before claiming those interactions are supported.
