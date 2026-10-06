# Shared instruction-fetch samples

## Finding and evidence

Persistent ARM/Thumb instruction buffering exposed duplicate memory reads in the interpreter.
The pipeline sampled the new lookahead instruction, then `Memory::begin_cpu_access` reread those bytes for bus history.
Thumb IWRAM refills also read their target pair once for history and again for instruction buffering.
With instruction-boundary scheduling, those reads normally saw identical bytes. They still represented the same fetch twice.

This refactor gives instruction buffering and supported bus-history updates a common captured sample.
It does not introduce new hardware formulas or change nominal cycle costs.

Previously reviewed evidence supports this separation:

- [NanoBoyAdvance at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh): `Run` fetches the next instruction before execution; refill helpers fetch the target pair.
- [ares at 6f6786e0](https://github.com/ares-emulator/ares/blob/6f6786e04f0822a3475463df284f313ab8518d51/ares/gba/cpu/memory.cpp): IWRAM updates local addressed lanes; EWRAM halfword reads drive duplicated halves.
- [Thumb BIOS research](thumb-bios-prefetch.md) and [Thumb open-bus rules](../hardware/cpu.md#thumb-unused-memory-data-reads): BIOS/OAM bus observations use an aligned word, not a universally duplicated halfword.

These sources were reviewed, not copied into the emulator.
No physical-hardware measurements or external-emulator differential runs were performed.

## Sample and consumers

`memory/fetch.rs` returns an `InstructionFetch` with:

- Fetch address and instruction state.
- Strict instruction result or deferred mapped-read error.
- Supported bus-word observation, when it does not require retained IWRAM lanes.

ARM samples use the fetched word for both instruction bits and bus observation.
Thumb samples retain only their selected halfword as instruction bits.
Supported 16-bit regions duplicate that halfword for bus observation.
BIOS/OAM samples read the adjacent halfword too, without rereading the instruction bytes.
Thumb IWRAM supplies the fetched halfword to the existing lane-retaining transaction instead of creating a full bus word.

For retained pipelines, the CPU passes only its new P+8 or P+4 sample to `begin_cpu_access`.
The later persistent IWRAM extension also passes newly sampled current/decode entries for cold fills.
It does not replay the bus observations of retained current/decode instructions.
The consumer performs no mapped memory read.
After successful control flow, the same target samples fill the instruction buffer and update local IWRAM history in either state.
Machine IRQ entry uses this refill path; BIOS vector fetches leave IWRAM lanes unchanged.

Instruction buffers and BIOS/IWRAM latches remain separate retained state.
Sampling alone changes no CPU access context, latch, device clock, DMA resume marker, or data-timing trace.
Instruction errors discard staged history and speculative buffer advances.
A retry takes a new lookahead sample while retaining the previously committed instruction pair.
Unavailable lookahead remains diagnostic only when consumed by an instruction or a data read that needs it.
Strict instruction reads never use protected BIOS or unused-memory data fallback.

## Current scope and limits

- Cold fills and cross-state local IWRAM history are now implemented in the [persistent-latch extension](iwram-bus-history.md).
- Thumb 16 MiB region crossings remain unsupported for bus snapshots.
- BIOS history commits after successful BIOS instruction execution; target refills do not establish additional BIOS history.
- All unused-memory data reads within an instruction still use its entry snapshot.
- DMA remains scheduled between whole instructions. Sampling does not add an interleaving point.
- Current-instruction and destination-based nominal timing remain unchanged.
- Per-access device updates, Game Pak prefetch, and exact refill/arbitration timing remain incomplete.

## Original tests

CPU tests split the existing internal fetch/execute helpers and mutate lookahead bytes between them.
ARM, narrow Thumb, wide Thumb, and IWRAM tests then require the originally captured values.
Those consumption tests failed with the previous duplicate-read path.
The split is a test-only ownership check, not a hardware DMA experiment.

Memory tests use explicit expected lane values across mapped regions and both Thumb alignments.
They also check target-pair consumption after mutation, BIOS snapshot retention, strict errors, short ROMs, and data-context isolation.
Tests verify that sampling adds no timing or history effects and that failed execution preserves committed history.
Existing instruction-buffer, bus-history, exception, DMA, and nominal timing tests remain regression coverage.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the previous Thumb-buffer reports exactly, including nominal cycle counts.
Debug and release reports also match for the same fixture paths.
These checks validate the refactor against existing behavior, not complete hardware bus or timing conformance.
