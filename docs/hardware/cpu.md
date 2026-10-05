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
Loads preserve CPSR and use normal load timing. Device clocks advance after the read, with IRQ sampling on the next step.
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

## ARM unused-memory data reads

During ARM execution, data reads from `0x00004000..0x01ffffff` and `0x10000000..0xffffffff` return an open-bus value.
Open bus means the bus retains previously driven data. GBATEK identifies this ARM value as the instruction word at PC+8.
These high addresses do not mirror BIOS, RAM, or cartridge addresses.

The interpreter snapshots mapped bytes at PC+8 before executing each ARM instruction.
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

- Thumb open bus uses the supported region rules below, including bounded sequential internal-RAM lane history.
- [BIOS-protected reads](bios.md#cpu-bios-read-protection) use separate retained ARM PC+8 or aligned Thumb PC+4 word snapshots.
- Unused/write-only I/O reads, general DMA-to-CPU bus handoff, and disabled-RAM reads are not modeled here.
  [DMA channel data](dma.md#retained-channel-data) is separate and never overrides a CPU snapshot.
- Missing BIOS, truncated ROM, unsupported I/O, and save-memory accesses retain their existing diagnostics.
- Unused-memory writes remain diagnostics rather than ignored hardware writes. Swaps cannot silently discard their write.
- There is no persistent instruction pipeline. Thumb IWRAM refills sample their target pair.
  DMA updates existing IWRAM continuation lanes at instruction boundaries, not during CPU accesses.

This subset lets public ARM test 362 complete. That test does not assert the loaded data value.
Original regressions check the values separately. See [the public ARM result](../public-arm-tests.md).

## Thumb unused-memory data reads

Thumb data reads use the same unused address ranges and lane rules as ARM reads above.
The snapshot depends on the executing code region. P below is the executing instruction address, not the visible r15 operand.

| Code region | Snapshot |
| --- | --- |
| External work RAM, palette RAM, video RAM, all three ROM windows | Halfword at P+4, repeated in both word lanes |
| BIOS and object attribute memory (OAM) | Full word at `(P+4) & ~3` |
| Internal work RAM (IWRAM) | PC+4 replaces one halfword in known sequential IWRAM history; the other lanes retain their values |
| Other code regions | Unsupported |

For BIOS/OAM, aligned instructions expose halfwords at P+4 and P+6.
Instructions at two-byte-only alignment expose halfwords at P+2 and P+4.
The two lanes can differ. A universal repeated-halfword rule would be incorrect.

Only the required mapped bytes are sampled. A 16-bit region needs two lookahead bytes, not four.
Normal memory mirrors apply to these bytes, including physical RAM/video-memory wrap within a mapped region.
If P+4 crosses a 16 MiB address-region boundary, the snapshot remains unknown.
This conservative diagnostic policy avoids guessing pipeline behavior during a region transition.
Missing lookahead or unknown history fails only an unused-memory load; ordinary instructions and mapped loads can still execute.

All loads in one instruction share the snapshot, including block loads and stack loads.
Normal sign extension, unaligned-load rotation, writeback, register aliases, and PC-load semantics still apply.
Snapshots add no nominal cycles. Host reads, instruction fetches, DMA, and unsupported stores do not gain fallback behavior.
Protected BIOS reads use separate retained history, not the current ROM/RAM snapshot.

GBATEK and nocash's [open-bus findings](https://www.ngemu.com/threads/gba-open-bus.170809/) document these region differences.
IWRAM needs prior bus lanes, including IWRAM data-read/write changes and possible DMA effects.
Do not substitute P+2 for that history: it is only the usual case, not a general rule.
The original Nintendo DS also differs from GBA-family IWRAM behavior; this core targets GBA.

This remains a bounded snapshot implementation, not a pipeline or a complete bus-history model.
Refill timing, self-modifying code, DMA-to-CPU transitions, cross-state IWRAM history, unused/write-only I/O, and disabled RAM remain incomplete.
Original regressions validate the documented formulas. No physical-hardware or independent public Thumb open-bus pass is claimed.

### Sequential IWRAM history

For consecutive Thumb instructions in IWRAM, the emulator retains a separate lane value and known-bit mask.
The PC+4 halfword fetch updates its addressed halfword before execution.
IWRAM data reads and writes then update only their addressed byte, halfword, or word lanes.
The recorded value is raw aligned bus data, before CPU rotation or sign extension.
Accesses to other regions do not replace the IWRAM latch.
See [the evidence and implementation scope](../research/iwram-bus-history.md).

History starts unknown. An unused-memory load requires all word lanes to be known, including for byte/halfword loads.
Ordinary instructions and mapped loads still execute with incomplete history.
Consecutive fetches can establish both halves. An IWRAM word read or write establishes the entire word.
Host inspection and setup writes do not seed or change this emulated history.

Each instruction stages its fetch and data-access changes.
Success commits history only for the next sequential Thumb IWRAM PC. Failure preserves the previous committed history.
Block-transfer data accesses stage updates in order; unused-memory reads still use one entry snapshot per instruction.
This preserves the existing atomic diagnostic policy without adding bus or device cycles.

Taken branches and PC-writing refills end the old continuation history, even when their target is the fallthrough address.
A successful refill into Thumb IWRAM establishes new history from its target pair, as described below.
Untaken conditional branches keep sequential history.
Other execution states/regions and accepted machine IRQs invalidate continuation history.
Successful DMA units update existing continuation lanes as described below.
A later exception return into Thumb IWRAM can establish new history through its refill.
Failed DMA units leave history unchanged. Discontinuous entry without an executed refill starts unknown.

### Thumb IWRAM refill history

After a successful refill instruction, the emulator uses the resulting PC and instruction state.
If the destination is Thumb IWRAM, it samples two halfwords in order: target T, then T+2.
Both accesses must remain in IWRAM. Normal physical RAM mirrors apply.
These samples establish both word lanes for execution at T. The first instruction then samples T+4 as usual.

This applies to ARM/Thumb BX, taken Thumb branches, BL suffixes, Thumb PC writes, and ARM status-restoring returns.
Saved Thumb state, not the target's low bit alone, controls exception-return sampling.
Stack and block-load returns finish their data accesses before the target samples.
Failed instructions do not sample a target or replace committed history.

Snapshots use strict mapped reads and add no nominal data or device cycles.
They update bus history only; the interpreter does not cache these instructions for execution.
Host inspection does not change captured lanes. A failed target fetch remains a later instruction diagnostic.
Cold direct startup, unsupported target states/regions, and region-crossing lookahead still have no inferred refill history.
DMA between refill and arrival updates this continuation's actual IWRAM access lanes before the target+4 sample.

The [refill evidence and scope](../research/iwram-bus-history.md#thumb-iwram-refill-extension) distinguish these samples from a full pipeline.
ARM-target refill history, sub-instruction DMA ordering, self-modifying instruction execution, and exact refill timing remain incomplete.

### DMA effects on Thumb IWRAM continuations

Within the instruction-boundary scheduler, DMA finishes before the next CPU PC+4 sample.
For an existing sequential or refill continuation, each successful DMA unit updates IWRAM source lanes, then IWRAM destination lanes.
Word accesses replace all lanes. Halfword accesses replace only the addressed halfword.
Other memory regions do not replace the local IWRAM value, even when DMA's channel data changes.
The resumed PC+4 fetch then replaces one halfword before the instruction's unused-memory snapshot is taken.

Channel halfword duplication is not a full-word IWRAM access.
A blocked source below work RAM does not drive IWRAM; an IWRAM destination still drives the halfword or word actually written.
Multiple units and channel preemption update lanes in access order, not channel-number order.
Host inspection/setup cannot change them.

History changes commit only after a unit succeeds. A failed unit retains the previous successful unit's history.
A failed resumed instruction discards its staged changes, not the completed DMA history.
DMA does not establish an expected PC at cold direct entry. Existing partially known continuations can gain known lanes.
PC discontinuities, unsupported state changes, IRQ entry, and region-crossing lookahead keep their existing conservative rules.

This implements local lane effects, not a general last-DMA-value override or a persistent pipeline.
Sub-instruction arbitration, DMA during CPU internal cycles, cold/cross-state IWRAM history, and exact resume timing remain incomplete.
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
Nominal timing remains one sequential code access, plus an internal cycle for register-specified shifts; there is no refill cost.
This fixes the mode-switch expectation in public `jsmolka/gba-tests` ARM test 234 and agrees with mGBA's shared ALU flag handling.
The interpreter has no fetch pipeline; instruction-state-changing forms and exact pipeline behavior remain hardware-unverified.

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

The timing model uses ARM7 cycle summaries:

- **S:** sequential memory access.
- **N:** non-sequential memory access.
- **I:** internal CPU cycle, always one clock cycle.

An ordinary arithmetic instruction costs 1S. A register-specified shift adds 1I, even for a zero shift amount.
Loads cost a code S access, a data N access, and 1I. Stores use N for code and data.
Block transfers charge N for the first data word and S for each following word.
Loads add 1I. Loading PC also adds the branch refill cost.
Branches, PC writes, software interrupts, and exception entry use a nominal 1N+2S code refill.
A skipped conditional instruction only pays its code S access.
Thumb `BL` charges 1S for its prefix and 1N+2S for its suffix.
Multiply costs vary with the incoming multiplier's upper bytes; accumulate and long forms add internal cycles.
ARM uses Rs for this calculation. Thumb multiply uses the incoming destination register.

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

### Nominal CPU resume after DMA

A successful DMA unit marks the next CPU instruction's nominal code access non-sequential.
This applies even when DMA only accesses RAM. Consecutive units retain one pending resume, not multiple penalties.
A non-refill instruction uses its normal width and current WAITCNT with N instead of S.
ARM word accesses retain an S cost for their second halfword.
Stores and ROM boundaries that already use N do not receive an additional cost.

Successful timed or untimed CPU execution consumes pending resume, including skipped conditions and internal-memory instructions.
Successful machine IRQ entry also consumes it. Failed CPU/DMA steps preserve it.
Host access, clock-only advancement, and HALT/STOP idle do not consume it.
A failed DMA unit cannot create pending resume.
DMA WAITCNT writes apply before resume; CPU WAITCNT writes still use the previous code-access settings.

Branch and exception refill summaries retain their existing destination-based `1N+2S` costs, without an added resume access.
The old-PC fetch is not modeled separately. This remains nominal timing, not a per-access pipeline.
See [the evidence and limitations](../research/dma-resume-timing.md).

Important timing limits:

- Code S/N counts follow instruction summaries, not a simulated fetch pipeline.
- Ordinary code costs use the current instruction address. The ARM open-bus PC+8 snapshot has no fetch timing or startup pipeline fill.
- PC writes use destination-region costs and the restored instruction width for nominal refill accesses.
- Refill cost calculation does not read target bytes. Separate Thumb IWRAM target-pair samples update bus history without extra cycles.
- Target-pair sampling cannot fail the branch early. An invalid branch target still fails on the following instruction fetch.
- Game Pak prefetch is not implemented. WAITCNT bit 14 is stored but does not accelerate execution.
- PHI and SRAM wait fields are stored; PHI output and SRAM mapping are not implemented.
- External work RAM timing is fixed. The undocumented memory-control register is not implemented.
- Exact DMA startup/resumption delays, display-bus contention, shared timer prescaler phase, and timer startup delays remain unmodeled.
