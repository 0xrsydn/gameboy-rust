# CPU: ARM7TDMI interpreter

Instruction semantics, processor modes, exceptions, and nominal timing as implemented in `crates/gba-core/src/cpu/`.

## Instruction demo and execution rules

The `--cpu-demo` command first runs the original instruction demo for 40 steps, starting at `0x08000000` in ARM state.
It counts r0 down from three while incrementing r1.
`SUBS` updates the flags. `BNE` repeats the loop until the zero flag is set.
`CMP` checks the result, and `MOVEQ` sets r2 to 42.
The program then stores 42 in work RAM at `0x02000000` and loads it into r4.
The program sets the stack pointer to `0x03000100` in internal work RAM.
A subroutine saves r4 and the link register using `STMDB sp!`.
It shifts r4 left into r5 and multiplies r4 by r5 into r6.
`SWP` writes 3528 to work RAM and returns the old value, 42, in r7.
The subroutine clears r4, then uses `LDMIA sp!` to restore r4 and return through the saved PC.
The program then uses `BX` to enter Thumb code at `0x08000064`.
A Thumb subroutine saves its return address, multiplies six by seven, and returns using `POP {pc}`.
The Thumb code stores 42 at `0x02000004`, checks it, and uses `BX` to return to ARM.
It finishes with `r0=42`, `r1=7`, `r2=42`, `r4=42`, `r5=84`, `r6=3528`, and `r7=42`.
The stack pointer returns to `0x03000100`.
The trace labels each step as `Arm` or `Thumb` and shows flags in `NZCV` order.
The final flags are `0110`.
Its last instruction branches to itself. The host stops after the fixed step count.

The command then runs an 11-step exception demo with a separate CPU and memory.
ARM and Thumb code each execute `SWI` and enter the ARM Supervisor handler through vector `0x08`.
The original test handler increments r10 and uses `MOVS pc, lr` to restore the caller's status.
The trace shows processor modes, instruction states, CPSR values, and two completed handler calls.
This test image is not a Nintendo BIOS and does not implement BIOS services.

A failed condition skips the instruction without changing registers or flags, except for advancing r15.
For subtraction, carry means no unsigned borrow occurred.
Logical flag-setting operations preserve overflow and take carry from the shifter.
Unshifted operands preserve the previous carry flag.
Arithmetic operations compute carry and overflow from the arithmetic result, not the shifter.

The CPU stores the next instruction address in r15.
In ARM state, ordinary operand reads add eight bytes. Register-specified shifts use a twelve-byte offset.
Storing r15 in ARM state also uses a twelve-byte offset.
Thumb empty-register-list stores use PC plus six; they must not use the ordinary Thumb operand offset.
Thumb operand reads use PC plus four. Literal loads and PC-relative addresses additionally align down to a word.
`BX` selects the instruction set; ordinary PC writes keep the current instruction set.
On ARM7, Thumb `POP {pc}` remains in Thumb even if the loaded address has bit zero clear.

`Cpu::new` starts in ARM System mode with interrupts enabled. It bypasses hardware boot.
`Cpu::at_reset()` starts at vector zero in ARM Supervisor mode, with IRQ and FIQ disabled.
Both constructors zero registers deterministically; real reset register values are not all defined.
`Cpu::instruction_set()`, `mode()`, `cpsr()`, and `spsr()` report the current processor state.
Each call to `Cpu::step(&mut memory)` executes one ARM word or one Thumb halfword, not one hardware cycle.
The two halves of Thumb `BL` execute separately. A standalone suffix uses the existing link register.

ARM single loads allow base and destination to share r0–r14, including with pre-indexed or post-indexed writeback.
This applies to `LDR`, `LDRB`, `LDRH`, `LDRSB`, and `LDRSH`.
The loaded value takes precedence over the updated base. Addresses and register offsets use incoming register values.
Pre-indexed loads access the adjusted address; post-indexed loads access the original address.
The discarded post-indexed writeback address is not a second memory access.
Normal alignment, word rotation, odd-halfword behavior, and sign extension still apply.
Loads preserve CPSR and use normal load timing. Machine timer reads sample at data-access completion; IRQ delivery waits until the next step.
Read diagnostics leave CPU state and device clocks unchanged under the existing atomic-error policy.
Stores with matching base/source still store the original base value before writeback.
PC writeback, byte/halfword PC destinations, and unimplemented user-transfer encodings remain diagnostics.
These alias rules implement ARM7 compatibility behavior, not a recommendation for portable ARM assembly.

Block transfers assign the lowest-numbered register to the lowest memory address.
They align addresses down without rotating loaded words, and preserve low base bits during writeback.
On ARM7, an empty register list transfers PC but uses a 64-byte span for addressing and writeback.
Thumb `STMIA Rb!, {}` stores executing PC+6 at the word-aligned base, then adds 64 to the unaligned base.
The shared empty `PUSH {}` path also stores PC+6, retaining its existing full-descending addressing and 64-byte stack adjustment.
The stored PC keeps bit 1; neither instruction word-aligns this value. Both stores advance execution by two bytes and preserve flags.
Empty Thumb loads still align the loaded PC to a halfword and remain in Thumb state.
`PUSH {lr}` is not an empty list and stores the original link register with a four-byte stack adjustment.
The PC+6 correction agrees with public Thumb test 229 and mGBA's shared empty-list store path.
These forms retain nominal single-word access costs and the existing atomic-error policy; exact bus timing remains unverified.
`LDM` suppresses writeback when the base register is in the list.
With writeback enabled, `STM` stores the old base only when that register is first in the list.
Otherwise, `STM` stores the updated base.

## ARM and Thumb instruction buffering

ARM and Thumb execution retain fetched instructions in a CPU-owned buffer, independent of later RAM contents.
Let W be the instruction width: four bytes for ARM, two for Thumb.
At cold entry, a successful step reads the current instruction P and samples P+W and P+2W before instruction effects.
During sequential execution, the current and next instructions come from retained results; only P+2W is newly sampled.
Success advances the buffer. CPU/DMA stores and host writes do not replace already buffered instructions.
The existing r15 convention is unchanged: it stores the next executing address, not the hardware fetch address.

A successful branch or PC-writing refill samples target T and T+W after instruction effects, using the resulting instruction state.
This includes taken branches, BX, PC loads, status-restoring returns, and software interrupts.
PC writes refill even when the destination equals fallthrough. Untaken conditional branches retain sequential buffering.
The first target instruction samples T+2W before execution. State transitions replace the buffer with the destination pair.
Thumb BL's prefix advances sequentially; only its suffix refills. Ordinary Thumb PC writes preserve Thumb state.
The buffer records both expected PC and instruction state. A state mismatch cannot reinterpret retained ARM words as Thumb halfwords.

Machine IRQ entry first samples and discards the old-state P+2W fetch, then captures the ARM vector pair.
The discarded fetch drives actual IWRAM lanes but does not execute an instruction or commit a BIOS instruction snapshot.
Missing source bytes cannot prevent IRQ entry. Missing vector bytes fail only when the handler instruction becomes current.
`Cpu::enter_exception` and `Cpu::take_interrupt` have no memory parameter: they invalidate the buffer and leave vector sampling to the next step.
An unexpected PC discontinuity starts from current mapped bytes rather than reusing a mismatched buffer.

Fetch samples use strict mapped reads, without protected-data or unused-memory fallback.
Unavailable lookahead is retained as a deferred diagnostic; it cannot fail an otherwise valid current instruction or branch.
A buffered fetch error fails only if execution reaches that slot. No automatic hardware abort is generated.
A failed instruction preserves all CPU state, including the old buffer; its speculative advance is discarded.
A successful branch discards abandoned-path errors and replaces them with target fetch results.

CPU clones and equality include the buffer and its next fetch access kind.
Architectural state checks are separate from full-state rollback checks in tests.
`Cpu::invalidate_pipeline()` explicitly discards buffered instructions without changing registers, flags, or device clocks.
Use it after debugger code repair or when attaching different memory to a retained CPU.
Invalidation also resets the CPU fetch kind to the nominal cold-start S policy; Memory's DMA override remains separate.
Do not call it for ordinary CPU/DMA stores: that would hide self-modifying-code behavior.

This is instruction buffering, not a complete timed fetch pipeline.
Sampling adds no data accesses or independent device-clock updates.
Code timing charges the source fetch and target pair once, with a DMA resume override on the source access.
Instruction buffering and supported bus history now consume the same mapped fetch samples.
Complete fetch-driven bus history, per-access device updates, and DMA arbitration remain incomplete.
See the [ARM evidence and diagnostic policy](../research/arm-instruction-buffer.md), [Thumb extension](../research/thumb-instruction-buffer.md),
and [shared fetch samples](../research/shared-fetch-samples.md).

## ARM unused-memory data reads

During ARM execution, data reads from `0x00004000..0x01ffffff` and `0x10000000..0xffffffff` return an open-bus value.
Open bus means the bus retains previously driven data. GBATEK identifies this ARM value as the instruction word at PC+8.
These high addresses do not mirror BIOS, RAM, or cartridge addresses.

The interpreter samples mapped bytes at PC+8 before executing each ARM instruction.
That one fetch supplies both the new instruction-buffer entry and the unused-memory snapshot.
All unused-memory reads within that instruction use the same snapshot.
Byte and halfword reads select their addressed lanes; the CPU applies normal rotation and sign extension.
Base writeback, register aliases, block loads, PC loads, and flags retain their existing instruction semantics.
Each data access pays the nominal unused-region cost of one cycle, plus normal code and internal costs.
Snapshot reads add no data accesses or device cycles. This is not a simulated prefetch pipeline or Game Pak prefetch.

The snapshot uses strict mapped reads, without recursive open-bus fallback.
If any snapshot byte is unavailable, an unused-memory data read retains its `Unmapped` diagnostic.
Missing lookahead does not fail ordinary instructions, mapped loads, or skipped conditional loads.
The CPU clears the access context after each instruction, including errors.
Host inspection, ROM-suite memory assertions, instruction fetches, and DMA do not inherit this context.

Limits remain explicit:

- Thumb open bus uses the supported region rules below, including persistent internal-RAM lane history.
- [BIOS-protected reads](bios.md#cpu-bios-read-protection) use separate retained ARM PC+8 or aligned Thumb PC+4 word snapshots.
- Unused/write-only I/O reads, general DMA-to-CPU bus handoff, and disabled-RAM reads are not modeled here.
  [DMA channel data](dma.md#retained-channel-data) is separate and never overrides a CPU snapshot.
- Missing BIOS, truncated ROM, unsupported I/O, and save-memory accesses retain their existing diagnostics.
- Unused-memory writes remain diagnostics rather than ignored hardware writes. Swaps cannot silently discard their write.
- ARM/Thumb instruction buffering is persistent, but these data snapshots remain a separate bounded model.
  Thumb IWRAM bus-history samples do not share storage with retained instructions.
  DMA updates IWRAM lanes at instruction boundaries, not during CPU accesses.

This subset lets public ARM test 362 complete. That test does not assert the loaded data value.
Original regressions check the values separately. See [the public ARM result](../public-arm-tests.md).

## Thumb unused-memory data reads

Thumb data reads use the same unused address ranges and lane rules as ARM reads above.
The snapshot depends on the newly fetched region at P+4, even when P lies in the preceding region.
P below is the executing instruction address, not the visible r15 operand.

| Fetch region | Snapshot |
| --- | --- |
| External work RAM, palette RAM, video RAM, all three ROM windows | Halfword at P+4, repeated in both word lanes |
| BIOS and object attribute memory (OAM) | Full word at `(P+4) & ~3` |
| Internal work RAM (IWRAM) | PC+4 replaces one halfword in the persistent IWRAM latch; the other lanes retain their values |
| Other code regions | Unsupported |

For BIOS/OAM, aligned instructions expose halfwords at P+4 and P+6.
Instructions at two-byte-only alignment expose halfwords at P+2 and P+4.
The two lanes can differ. A universal repeated-halfword rule would be incorrect.

The pipeline's new fetch supplies the bus snapshot without rereading its instruction bytes.
BIOS/OAM samples include the other halfword of the aligned bus word, but retain only the selected halfword as an instruction.
Only the required mapped bytes are sampled. A 16-bit region needs two lookahead bytes, not four.
Normal memory mirrors apply to these bytes, including physical RAM/video-memory wrap within a mapped region.
Crossing a 16 MiB region boundary does not by itself make the snapshot unknown.
Palette-to-VRAM fetches repeat the new halfword; VRAM-to-OAM fetches expose the new aligned word.
OAM-to-ROM and mapped ROM-window crossings repeat the fetched ROM halfword.
An EWRAM-to-IWRAM fetch drives its addressed local lane and requires the other lanes to be known.
For example, P=`0x02fffffc` needs prior high-lane history; P=`0x02fffffe` can establish both halves through its fill/refill and lookahead.
Missing lookahead, unknown IWRAM lanes, and unsupported I/O fetch observations still produce diagnostics for unused-memory loads.
Ordinary instructions and mapped loads can execute without a usable unused-memory snapshot.
See [the boundary evidence and original tests](../research/fetch-region-boundaries.md).

All loads in one instruction share the snapshot, including block loads and stack loads.
Normal sign extension, unaligned-load rotation, writeback, register aliases, and PC-load semantics still apply.
Snapshots add no nominal cycles. Host reads, instruction fetches, DMA, and unsupported stores do not gain fallback behavior.
Protected BIOS reads use separate retained history, not the current ROM/RAM snapshot.

GBATEK and nocash's [open-bus findings](https://www.ngemu.com/threads/gba-open-bus.170809/) document these region differences.
IWRAM needs prior bus lanes, including IWRAM data-read/write changes and possible DMA effects.
Do not substitute P+2 for that history: it is only the usual case, not a general rule.
The original Nintendo DS also differs from GBA-family IWRAM behavior; this core targets GBA.

Bus history remains bounded despite sharing samples with the persistent instruction buffer.
Refill timing, general DMA-to-CPU transitions, unused/write-only I/O, and disabled RAM remain incomplete.
Original regressions validate the documented formulas. No physical-hardware or independent public Thumb open-bus pass is claimed.

### Sequential IWRAM history

The emulator retains a separate IWRAM lane value and known-bit mask, independent of CPU state or executing region.
Every successful ARM IWRAM fetch drives a word; every Thumb IWRAM fetch drives its addressed halfword.
IWRAM data reads and writes update their addressed byte, halfword, or word lanes, including accesses from non-IWRAM code.
The recorded value is raw aligned bus data, before CPU rotation or sign extension.
Accesses to other regions, state changes, and exception entry do not erase this local latch.
See [the evidence and implementation scope](../research/iwram-bus-history.md).

History starts unknown. An unused-memory load requires all word lanes to be known, including for byte/halfword loads.
Cold pipeline filling samples the current instruction, next instruction, and lookahead in order.
A mapped Thumb IWRAM fill therefore establishes both halves before the first data read.
A retained pipeline advances only its new lookahead; old instruction slots do not drive the bus again.
Debugger invalidation does not erase the latch, but the following cold fill updates it from current mapped bytes.
Host inspection and setup writes do not seed or change this emulated history.

Each instruction stages all new fetches and data-access changes.
Success commits history regardless of the resulting PC, instruction state, or region. Failure preserves the previous committed history.
Block-transfer data accesses stage updates in order; unused-memory reads still use one entry snapshot per instruction.
This preserves the existing atomic diagnostic policy without adding bus or device cycles.
Missing or unsupported Thumb lookahead still leaves that instruction's unused-memory snapshot unknown.
Mapped boundary fetches into IWRAM use its resulting local lanes, even while the current instruction executes elsewhere.
Failed or other-region observations do not erase independently known IWRAM lanes.

### Thumb IWRAM refill history

After a successful refill instruction, the emulator uses the resulting PC and instruction state.
Thumb target fetches occur at T and T+2; ARM target fetches occur at T and T+4.
Each successful IWRAM fetch drives its own lanes, in order. Other-region or unavailable fetches do not erase local history.
Normal physical RAM mirrors apply.
A complete Thumb IWRAM target pair establishes both word lanes; the first target instruction then samples T+4.
An ARM IWRAM target pair leaves the second fetched word in the latch.

This applies to BX, taken branches, BL suffixes, PC writes, PC loads, and status-restoring returns.
Saved instruction state, not the target's low bit alone, controls exception-return sampling.
Stack and block-load returns finish their data accesses before the target samples.
Failed instructions do not sample a target or replace committed history.

Target samples use strict mapped reads and add no nominal data or device cycles.
The same captured target pair fills the CPU buffer and updates IWRAM history; no second target read occurs.
A failed target fetch remains a later instruction diagnostic.
IRQ entry updates IWRAM lanes if its discarded old-state fetch addresses IWRAM.
Its subsequent ARM BIOS vector pair does not erase those lanes.
A BIOS handler can still change those lanes through actual IWRAM data accesses.

The [refill evidence and scope](../research/iwram-bus-history.md#thumb-iwram-refill-extension) distinguish these samples from a fully timed pipeline.
Sub-instruction DMA ordering and complete bus sequencing remain incomplete.

### DMA effects on Thumb IWRAM continuations

Within the instruction-boundary scheduler, DMA finishes before the next CPU fetch.
Each successful DMA unit updates IWRAM source lanes, then IWRAM destination lanes, even before the first CPU instruction.
Word accesses replace all lanes. Halfword accesses replace only the addressed halfword.
Other memory regions do not replace the local IWRAM value, even when DMA's channel data changes.
A resumed Thumb PC+4 fetch then replaces one halfword before the instruction's unused-memory snapshot is taken.
A cold fill instead drives all its newly fetched instructions.

Channel halfword duplication is not a full-word IWRAM access.
A blocked source below work RAM does not drive IWRAM; an IWRAM destination drives the actual halfword or word written.
Multiple units and channel preemption update lanes in access order, not channel-number order.
Host inspection/setup cannot change them.

History changes commit only after a unit succeeds. A failed unit retains the previous successful unit's history.
A failed resumed instruction discards its staged changes, not the completed DMA history.
No expected PC or Thumb continuation is needed to record DMA activity.

This implements local lane effects, not a general last-DMA-value override.
Sub-instruction arbitration, DMA during CPU internal cycles, and exact resume timing remain incomplete.
See [the source evidence and limits](../research/iwram-bus-history.md#dma-continuation-extension).

## Processor status and exceptions

User and System share registers. Each exception mode has its own stack pointer, link register, and SPSR.
Fast interrupt mode (`FIQ`) also banks r8–r12. Other modes share those registers.
`MRS` reads status. `MSR` writes selected fields; User mode can only change CPSR arithmetic flags.
Reserved status bits read as zero. Unsupported status fields have no effect.
Invalid CPSR modes and attempts to change its Thumb bit through `MSR` return diagnostics.
Use `BX` or an exception return to change instruction state.

An exception saves CPSR in the destination mode's SPSR and writes that mode's link register.
Entry selects ARM state, masks interrupt requests (`IRQ`), and branches to the exception vector.
FIQ entry also masks FIQ. Ordinary IRQ entry preserves the FIQ mask.
`MOVS pc, lr`, `SUBS pc, lr, #offset`, and `LDM` with S and PC restore CPSR from SPSR.
Return alignment follows the saved instruction state, not target bit zero.
Result-writing PC status returns fail in User/System modes because those modes have no SPSR.
An SPSR can contain invalid mode bits, but a return using those bits fails without changing CPU state.

ARM `TST`, `TEQ`, `CMP`, and `CMN` encodings with unused destination bits set to R15 restore CPSR when an SPSR exists.
These compare/status forms do not write the arithmetic result to PC. Execution continues at the next sequential address.
In User/System modes, they perform ordinary test/compare flag updates without a mode change.
Invalid saved modes still produce an atomic diagnostic. Failed conditions do not restore or validate status.
Nominal timing uses the incoming code access kind, plus an internal cycle for register-specified shifts; there is no refill cost.
This fixes the mode-switch expectation in public `jsmolka/gba-tests` ARM test 234 and agrees with mGBA's shared ALU flag handling.
Instruction buffering is discarded on a state change without an ordinary refill.
Instruction-state-changing compare forms and exact pipeline behavior remain hardware-unverified.

Other S-bit block transfers access User registers while using the current mode's base register.
User-bank writeback, S-bit empty lists, and User-mode S-bit transfers return diagnostics.
These restrictions avoid unpredictable or unverified forms.

`Cpu::enter_exception` explicitly supports software interrupt, undefined instruction, prefetch abort, data abort, IRQ, and FIQ entry.
For synchronous exceptions, call it with PC at the faulting instruction.
For IRQ/FIQ, call it between instructions, with PC at the next instruction.
`Cpu::take_interrupt(irq, fiq)` samples supplied interrupt lines between steps and respects masks and FIQ priority.
`Machine` connects timer, display, and DMA requests to the GBA IRQ line. GBA devices do not generate FIQ.
Memory and unsupported-instruction errors do not automatically enter exceptions.

`Memory::with_bios(rom, bios)` accepts exactly 16 KiB of caller-supplied vector code.
`Memory::new(rom)` leaves the BIOS area unmapped.
Without vector code, `SWI` enters Supervisor mode, then the next fetch reports an unmapped-memory error.
Neither constructor provides Nintendo BIOS services or initializes BIOS-managed RAM.
`bios::boot(rom)` separately opts into the original firmware subset described in [bios.md](bios.md).

## Instruction and bus timing

The timing model distinguishes these ARM7 cycle types:

- **S:** sequential memory access.
- **N:** non-sequential memory access.
- **I:** internal CPU cycle, always one clock cycle.

Every instruction charges its source fetch using the incoming access kind retained by the CPU buffer.
Ordinary arithmetic and immediate shifts leave the next fetch S.
Register-specified shifts add 1I, even for a zero shift amount, and leave the next fetch N.
Loads and stores start their data accesses with N and leave the next source fetch N.
Block transfers use N for the first data word and S for each following word.
Loads add 1I. Loading PC also fetches a target pair, which restores S for subsequent target execution.
Branches, PC writes, software interrupts, and IRQ entry charge one source fetch plus a target N+S pair.
The source fetch uses the incoming state at P+2W. The target pair uses the resulting state at T and T+W.
The first target instruction charges T+2W when it executes; the refill does not charge it early.
A skipped conditional instruction only pays its incoming code access and leaves the following fetch S.
Thumb `BL` charges one source fetch for each half; only its suffix adds the target N+S pair.
Multiply costs vary with the incoming multiplier's upper bytes; accumulate and long forms add internal cycles.
ARM uses Rs for this calculation. Thumb multiply uses the incoming destination register.
Multiply internal cycles leave the following fetch N. Swaps also leave N after their data/internal work.
Successful target refills end with S, overriding sequence breaks from preceding data/internal work.
IRQ entry uses the interrupted buffer's incoming kind before replacing that buffer.

Source code costs use the new instruction fetch address: P+8 for ARM and P+4 for Thumb.
The incoming instruction state selects the width. The sampled fetch address selects the memory region and ROM wait-state window.
A 128 KiB ROM boundary therefore forces N timing when lookahead reaches it, not when execution reaches it later.
This applies to cold and retained pipelines without adding separate startup-fill costs.
See [fetch-address timing](../research/fetch-address-timing.md), [refill timing](../research/refill-fetch-timing.md),
and [persistent access kinds](../research/fetch-access-sequencing.md) for evidence and limits.

Data costs use the addresses and widths of actual CPU bus calls, after alignment handling.
BIOS, internal work RAM, OAM, and supported I/O accesses cost one cycle.
External work RAM costs 3 cycles for byte/halfword accesses and 6 for words.
Palette and video RAM cost 1 cycle for byte/halfword accesses and 2 for words, without display contention.
ROM costs include one cycle plus the configured wait states for each 16-bit transfer.
A ROM word uses two halfword accesses; its second halfword always uses sequential timing.
Accesses at 128 KiB ROM boundaries force non-sequential timing for the first halfword.

`WAITCNT` resets to zero and supports byte, halfword, and word access.
The writable mask is `0x5fff`; the Game Pak type flag reads as GBA, and the upper halfword reads as zero.
A CPU write to WAITCNT does not retroactively change that instruction's code-access cost.
The new settings apply to subsequent code accesses.

Timed CPU execution uses an ordered `CpuTiming` transaction: source fetch, actual data accesses, internal work, then target-pair fetches.
A failed instruction discards the entire transaction. Target samples do not add duplicate fetch or data costs.
IRQ entry records its source fetch and vector pair in the same path.
Machine timer state advances through these events and commits only after success. Other device clocks still advance in bulk afterward.
This is not a complete per-cycle scheduler.
The transaction also stages the nominal Game Pak prefetch queue described below.

### Game Pak opcode prefetch

WAITCNT bit 14 enables an eight-halfword timing queue, separate from retained CPU instructions and bus-history values.
ROM opcode misses start a stream. Internal work and non-cartridge accesses advance an active stream.
Matching queued ARM words or Thumb halfwords cost one cycle; partial words wait for the remaining halfword work.
The CPU-side delivery cycle also advances active background work. A CPU N request can still hit the queue.
ROM data accesses and nonmatching ROM code requests cancel the stream.
Cancellation adds one cycle when an active halfword has one cycle remaining.
Full-buffer production stops until the entries drain; the next empty demand restarts with N timing.
Background production pauses before 128 KiB boundaries, where demand uses forced-N timing.

The queue stores no instruction bytes and performs no background memory reads.
Missing lookahead and branch targets therefore keep their existing deferred diagnostic behavior.
Successful CPU transactions commit queue progress; failures discard it.
`Cpu::step` maintains the same queue as `Cpu::step_timed` when prefetch is enabled, without advancing device clocks.
Cold or invalidated CPU pipelines clear staged queue metadata and retain nominal S startup timing.

Changes to ROM wait fields or prefetch enable clear queue metadata after the WAITCNT write.
PHI/SRAM-only changes preserve it. Exact hardware live-reconfiguration behavior remains unverified.
RAM DMA and HALT supply idle cartridge time; cartridge DMA cancels the stream. STOP freezes queue progress.
Host reads and `Memory::advance_cycles` do not advance the queue; machine steps account for progress separately.

These are source-backed nominal rules, not a hardware conformance claim.
See [queue evidence, regression coverage, and limits](../research/gamepak-prefetch.md).
The [original cancellation probe](../research/prefetch-cancellation.md) matches published read interval totals at instruction boundaries.
Its unadjusted timer samples now also match after [ordered timer accesses](../research/ordered-timer-access.md).

### Nominal CPU resume after DMA

A successful DMA unit marks the next CPU instruction's nominal code access non-sequential.
This applies even when DMA only accesses RAM. Consecutive units retain one pending resume, not multiple penalties.
The source request uses its actual lookahead address, incoming width, and current WAITCNT with N instead of S.
A matching enabled prefetch stream can satisfy that request without paying the raw N cost.
This includes branches and accepted IRQ entry; target pairs still use N then S.
ARM word accesses retain an S cost for their second halfword.
CPU history and ROM boundaries that already require N do not receive an additional access.
A completed load or store can establish a new N requirement after consuming the DMA override.

Successful timed or untimed CPU execution consumes pending resume, including skipped conditions and internal-memory instructions.
Successful machine IRQ entry also consumes it. Failed CPU/DMA steps preserve it.
Host access, clock-only advancement, and HALT/STOP idle do not consume it.
A failed DMA unit cannot create pending resume.
DMA WAITCNT writes apply before resume; CPU WAITCNT writes still use the previous code-access settings.

A resumed refill can have two N accesses: the old-PC source fetch and the first target fetch.
The resume changes the source access kind; it does not append another access.
This remains nominal timing, not a per-access pipeline.
See [the evidence and limitations](../research/dma-resume-timing.md).

Important timing limits:

- Source S/N kinds follow the preceding successful instruction; data timing still uses an instruction-local trace.
- Code costs follow actual source and target-pair addresses, but cold current/decode fills have no separate startup charge.
  Cold entry, debugger invalidation, and PC/state mismatches use nominal S, not hardware-validated startup timing.
- Refill cost calculation does not read target bytes. ARM/Thumb instruction-buffer and local IWRAM bus-history samples add no extra cycles.
- Target-pair sampling cannot fail the branch early. An invalid branch target still fails on the following instruction fetch.
- Game Pak prefetch is nominal. Full-buffer restart, page boundaries, cancellation, and live WAITCNT changes need independent hardware validation.
- PHI and SRAM wait fields are stored; PHI output and SRAM mapping are not implemented.
- External work RAM timing is fixed. The undocumented memory-control register is not implemented.
- Exact DMA startup/resumption delays, display-bus contention, and timer startup/register-write delays remain unmodeled.
