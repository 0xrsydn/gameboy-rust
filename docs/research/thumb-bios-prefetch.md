# Thumb BIOS prefetch snapshots

## Decision

Retain the full mapped word at `(executing PC + 4) & !3` after a successful Thumb BIOS instruction.
Keep ARM's existing PC+8 rule. Keep this BIOS history separate from general unused-memory open bus.

BIOS has a 32-bit bus. A Thumb instruction fetch selects a halfword, but the BIOS bus drives both word lanes.
This gives the following snapshot for an instruction at address P:

| Executing address | Low halfword | High halfword |
| --- | --- | --- |
| P aligned to four bytes | BIOS[P+4] | BIOS[P+6] |
| P aligned to two bytes only | BIOS[P+2] | BIOS[P+4] |

The two halfwords need not match. Do not repeat the fetched halfword, zero-extend it, or read an unaligned word at P+4.
Do not use ARM's PC+8 offset for Thumb execution.

## Evidence and confidence

[GBATEK](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm) documents two related behaviors:

- Protected BIOS reads return the most recently fetched BIOS opcode.
- Thumb open bus while executing in BIOS exposes the two aligned word lanes shown above.

Its Thumb table describes unused-memory reads, not a dedicated Thumb protected-BIOS test.
The retained-word rule combines that bus-width evidence with the documented BIOS latch behavior.
We have not measured it on physical hardware.

[NanoBoyAdvance's bus implementation](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/bus/bus.cc) corroborates the full-word latch.
`ReadBIOS` aligns a permitted address down to four bytes and updates its BIOS latch with a full word.
It then selects the requested lanes. A halfword access does not reduce the retained latch to 16 bits.
Its [ARM7TDMI execution loop](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) fetches the Thumb PC+4 halfword before dispatching the instruction.
This is a source comparison, not a differential emulator run. No implementation code was copied.

[mGBA's memory implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/memory.c) was also reviewed.
Its region-exit path saves `cpu->prefetch[1]`, while its [Thumb execution loop](https://github.com/mgba-emu/mgba/blob/master/src/arm/arm.c) fetches a halfword.
That path does not independently establish both retained word lanes. Its general Thumb BIOS open-bus path also notes an alignment approximation.
Do not treat agreement between all emulators, or exact hardware conformance, as established.

## Implementation scope

`Memory::begin_cpu_access` samples before execution, using the incoming instruction state and strict mapped bytes.
`end_cpu_access` commits BIOS history only after success. Failed instructions preserve previous history.
Thumb execution outside BIOS neither initializes nor replaces BIOS history.
Host reads remain raw inspection. CPU data loads outside BIOS select lanes from retained history as before.
Snapshots add no data accesses or nominal cycles.

The snapshot does not create a persistent instruction pipeline.
It does not model refill-time latches, BIOS data-access ordering within an instruction, DMA bus transitions, or general Thumb open bus.
At BIOS boundaries, unavailable lookahead invalidates history under the existing diagnostic policy.
A branch can still succeed; a later protected read reports unknown history rather than exposing stale bytes.
This boundary policy is conservative, not a hardware claim.

## Regression coverage

Original tests in `crates/gba-core/src/cpu/thumb_bios_access_tests.rs` cover:

- Both Thumb instruction alignments and both output instruction states.
- Full words, byte lanes, halfwords, sign extension, rotation, and all processor modes.
- Sequential history, ARM/Thumb transitions, SWI entry, and POP-to-PC exits with RAM stack data.
- Raw reads inside BIOS and protected reads outside it.
- The last complete fetch word, unavailable lookahead, and failure retention.
- Separate host reads, unchanged general Thumb open-bus diagnostics, nominal costs, and device progression.

## Validation result

The Thumb boundary-read regressions failed on the old history-invalidation path and pass with aligned-word snapshots.
Darwin arm64 workspace and core/demo tests pass in debug and release builds.
Preparation tests, formatting, lint checks, rustdoc, native ROM windows, and graphics smoke tests also pass.
The pinned public ARM, Thumb, memory, and BIOS reports match their previous passing reports exactly.
Debug and release reports also match each other for the same fixture paths.

The pinned public BIOS ROM uses ARM firmware paths. Its unchanged pass is regression evidence, not independent Thumb BIOS coverage.
Full pipeline timing and commercial-game compatibility remain outside this change.
