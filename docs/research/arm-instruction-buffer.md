# Persistent ARM instruction buffering

## Evidence and scope

The [ARM7TDMI instruction-pipeline description](https://developer.arm.com/documentation/dvi0027/b/arm7tdmi/instruction-pipeline) identifies fetch, decode, and execute stages.
The [ARM7TDMI technical reference manual](https://documentation-service.arm.com/static/5e8e1323fd977155116a3129) describes simultaneous execution, decoding, and fetching.
For ARM state, the executing instruction's visible PC is eight bytes ahead.
The [branch cycle sequence](https://support.arm.com/documentation/ddi0029/g/instruction-cycle-timings/branch-and-branch-with-link) fetches the destination, then destination plus instruction width.

[NanoBoyAdvance at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) makes the retained-word ordering explicit:

- `ReloadPipeline32` reads target T, then T+4.
- `Run` takes the current instruction from its retained first slot.
- It shifts the second slot forward and reads the new fetch word before executing the current instruction.
- Taken PC writes reload the target pair instead of retaining the sequential continuation.

The resulting ARM sequence fetches P+8 before the instruction at P changes memory.
Consequently, a store to P+4 or P+8 cannot change those already fetched instructions without a refill.
A later store-visible word, such as P+12, is fetched on a subsequent step.
DMA writes likewise do not replace words already inside the CPU buffer.

These are source-backed instruction-buffer rules, not a claim of complete ARM7 bus timing.
No external source code or test ROM was imported. No physical-hardware or external-emulator differential run was performed.

## Implementation

`cpu/pipeline.rs` owns a private two-slot continuation with its expected execution PC and instruction state.
The later [Thumb extension](thumb-instruction-buffer.md) uses this same structure for halfwords.
Each slot retains either a mapped word or its strict fetch error.
`Cpu` owns and clones this state; full CPU equality includes it.

At a cold or discontinuous ARM entry, fetch the current word and sample P+4/P+8.
At a matching continuation, use its first word for execution and retain its second word for the next step.
Sample P+8 before instruction effects. Keep that prospective continuation local until execution succeeds.
A failed current fetch or instruction preserves the previous buffer exactly.

Successful non-refill ARM execution commits the next pair.
A successful refill into ARM instead samples T and T+4 after data transfers and status restoration.
This includes ARM/Thumb BX, ARM PC writes and loads, taken branches, exception returns, and SWI vectors.
A refill to the sequential address still discards the previous sequential pair.
Untaken branches retain the pair. A transition to Thumb now replaces it with a Thumb target pair.

Machine IRQ entry captures its vector pair immediately after exception entry.
The standalone exception APIs have no Memory argument, so they invalidate the old pair and leave the vector cold.
This distinction is explicit rather than retaining unrelated words at a coincident PC.

`Memory::fetch_instruction` reads aligned mapped instructions without CPU data context, timing traces, or bus-history changes.
Normal memory mirrors apply. Unmapped, truncated, and misaligned fetches retain their existing errors.
No BIOS-protected data value or general open-bus value can become a synthetic instruction through this path.

## Diagnostics and debugger behavior

An unavailable lookahead word is a deferred fetch diagnostic, not an immediate failure of the current instruction.
A valid branch can discard an unavailable sequential word. An unavailable target does not fail the branch early.
Execution reports a buffered error only when that slot becomes current.
Unsupported instruction encodings remain words until decode; they also do not fail preceding instructions.

The atomic-error policy preserves the entire old CPU buffer on failure.
A host repair of a buffered unsupported instruction does not silently change that retained word.
`Cpu::invalidate_pipeline()` explicitly clears the buffer for debugger repair or attachment to different memory.
It changes no architectural registers, memory, clocks, or separate BIOS/IWRAM bus history.
Ordinary CPU/DMA stores and host inspection never invoke it automatically.

A copied CPU retains its own instruction-buffer state. A copied CPU attached to unrelated memory must be invalidated explicitly.
Existing successful-instruction tests now distinguish architectural equality from full-state equality.
Failed-instruction tests continue to assert full-state preservation.

## Remaining limits

This implements persistent ARM instruction words, not a complete cycle-by-cycle pipeline.
Cold fill and target-pair samples do not add nominal cycles or data accesses.
The later [refill timing extension](refill-fetch-timing.md) replaces destination-only costs with source-fetch and target-pair costs.
IRQ entry also samples its discarded old-state fetch before the vector pair.
The later [fetch-address timing extension](fetch-address-timing.md) selects non-refill code cost from the newly sampled address.
The later [sequencing extension](fetch-access-sequencing.md) stores the next fetch kind in the instruction buffer.
Cold-fill startup costs and per-access scheduling remain incomplete.

ARM PC+8 open bus and protected BIOS readback remain separate snapshots.
The [shared fetch sample](shared-fetch-samples.md) supplies the instruction-buffer entry and supported bus snapshots without duplicate reads.
No unified per-access bus model exists yet.
ARM IWRAM target samples now drive the [persistent local latch](iwram-bus-history.md).
Complete BIOS and general-bus history remain unimplemented.
Instruction-state-changing compare quirks, Game Pak prefetch, and DMA arbitration remain incomplete.
DMA still runs between whole instructions, not during their data accesses or internal cycles.

## Original regression coverage

Original programs test stores to the two prefetched words and the next unfetched word.
Other tests cover host/DMA writes, taken and untaken branches, PC writes to fallthrough, target-pair sample order, and ARM/Thumb transitions.
SWI/IRQ entry and status-restoring returns verify refill integration.
Further tests check deferred fetch errors, discarded paths, failed-instruction rollback, explicit invalidation, CPU cloning, mirrors, and alignment.
Timed and untimed execution retain identical buffers and nominal costs.
The self-modifying cases failed under the previous direct-fetch interpreter before implementation.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the previous passing reports exactly, including nominal cycle counts.
These suites remain regression checks, not complete instruction-pipeline or hardware-timing conformance tests.
