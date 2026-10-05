# Persistent Thumb instruction buffering

## Evidence and scope

The [ARM7TDMI pipeline description](https://developer.arm.com/documentation/dvi0027/b/arm7tdmi/instruction-pipeline) identifies fetch, decode, and execute stages.
The [technical reference manual](https://documentation-service.arm.com/static/5e8e1323fd977155116a3129) describes Thumb's two-byte instruction width and visible PC offset of four bytes.
The [Thumb BL cycle sequence](https://support.arm.com/documentation/ddi0210/c/Instruction-Cycle-Timings/Thumb-branch-with-link) distinguishes the sequential prefix from the branching suffix.

[NanoBoyAdvance at 55b5cf0a](https://github.com/nba-emu/NanoBoyAdvance/blob/55b5cf0ae3d929582ac5bfd486558173502b8354/src/nba/src/arm/arm7tdmi.hh) provides an implementation cross-check:

- `Run` takes the retained current instruction and shifts the next instruction forward.
- In Thumb state, it reads the next halfword before executing the current instruction.
- `ReloadPipeline16` reads target T, then T+2. The target instruction later fetches T+4.
- `ReloadPipeline32` uses ARM's four-byte width instead.

Thus, execution at P retains the halfwords at P+2 and P+4 before instruction effects.
A store cannot replace these retained instructions. It can affect P+6, which is fetched later.
A taken PC write discards the sequential pair, even when its destination equals fallthrough.
The BL prefix retains its suffix; only the suffix refills the target pair.

This extends the [ARM instruction buffer](arm-instruction-buffer.md), not the bus scheduler.
No external source code or test ROM was imported. No physical-hardware or external-emulator differential run was performed.

## Implementation

`cpu/pipeline.rs` shares one two-slot buffer between ARM and Thumb.
Each buffer records its expected execution PC and instruction state.
Both must match before retained instructions can be reused.
Thumb halfwords are zero-extended in the slots; upper bits do not hold another instruction.

Cold execution reads the current instruction and samples the next two instructions.
Sequential execution uses retained results and samples only P+4 in Thumb state.
The prospective continuation commits only after successful execution.
Refills sample the target pair after data transfers and status restoration, using the resulting instruction state.
ARM-to-Thumb BX and saved-state exception returns therefore capture T and T+2, not T and T+4.
Thumb-to-ARM transitions and SWI/IRQ vectors capture ARM pairs.

`Memory::fetch_instruction` uses strict mapped reads with the selected instruction width.
A Thumb fetch reads exactly two bytes, even in a region whose separate bus snapshot uses a full word.
Memory mirrors apply. Unmapped and misaligned instruction reads retain diagnostics rather than using data open bus.
Lookahead errors remain deferred until their slot executes; a branch can discard them.

Failed execution preserves the previous buffer exactly. Newly sampled lookahead does not commit.
CPU stores, DMA transfers, and host writes never replace retained instructions automatically.
`Cpu::invalidate_pipeline()` explicitly discards retained instructions for debugger repair or attachment to different memory.
It does not clear separate BIOS or IWRAM bus history.
CPU clones own independent buffers; full CPU equality includes them.

## Separation from bus history and timing

Instruction retention and data-bus snapshots remain separate models.
Cold instruction-buffer filling does not make unknown IWRAM bus lanes known.
Successful DMA can update existing IWRAM bus lanes without changing retained CPU instructions.
Refill bus-history sampling and instruction sampling currently perform separate strict mapped reads.

Buffer samples add no nominal cycles or data accesses.
Timed and untimed execution use the same buffer logic.
Destination-based refill summaries and one-shot non-sequential CPU resume costs remain unchanged.
Unified fetch-driven bus history, exact region crossings, per-access timing, Game Pak prefetch, and sub-instruction DMA arbitration remain incomplete.
Instruction-state-changing compare quirks also remain hardware-unverified.

## Original regression coverage

Original programs cover:

- CPU byte/halfword stores to P+2, P+4, and the later P+6 instruction.
- Host writes, RAM/video mirrors, and halfword/word DMA writes.
- Taken and untaken branches, PC writes to fallthrough, and standalone BL suffixes.
- BL prefix retention and suffix refill, with unchanged nominal costs.
- ARM-to-Thumb target pairs at both halfword alignments and SWI/IRQ saved-state returns.
- Failed data loads, discarded speculative advances, deferred short-ROM errors, and strict target diagnostics.
- Debugger invalidation, independent clones, PC/state mismatches, and alignment errors.
- Equal timed/untimed buffers without additional nominal cycles.

The retention regressions failed with the previous direct-fetch Thumb interpreter before implementation.
Existing bus-history tests now invalidate deliberately repaired code or repair a data address without changing the retained instruction.
Successful instruction-semantic tests compare architectural state separately; failed-step tests still compare complete CPU state.

## Validation result

Workspace and core/demo tests pass in debug and release on Darwin arm64.
Formatting, lint checks, rustdoc, preparation tests, native ROM windows, and graphics smoke modes pass.
Public ARM, Thumb, memory, and BIOS reports match the previous ARM-buffer reports exactly, including nominal cycle counts.
Debug and release reports also match for the same fixture paths.
These checks do not establish full hardware pipeline or timing conformance.
