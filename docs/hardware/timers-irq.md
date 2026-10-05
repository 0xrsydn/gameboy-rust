# Timers, interrupts, and HALT

## Timers, interrupt registers, and the machine clock

`Machine::new(cpu, memory)` combines the CPU and memory bus.
`Machine::step()` first services one ready DMA data unit, keeping the CPU paused.
If HALT is waiting and no DMA is ready, it advances device clocks to the next event without executing CPU code.
Otherwise, it samples the IRQ line, including the CPU's interrupt mask, then enters IRQ mode or executes one instruction.
Each successful step advances display and timer clocks by its nominal cost.
`Machine::last_timing()` reports separate code, data, internal, and idle cycle totals for the last successful step.
`StepTiming::idle_cycles` counts device-clock cycles during HALT; it does not count CPU work.
IRQ entry is a separate step; the vector instruction runs on the following step.
A device event during a step can trigger IRQ entry once no DMA is ready and CPU masks allow delivery.
Bus writes take effect before the step's device-clock update.
`StepKind::Dma { channel }` identifies a DMA unit. `StepKind::HaltIdle` identifies one bounded HALT clock advance.
The other step kinds remain `Instruction` and `IrqEntry`.
`MachineError::Cpu` and `MachineError::Dma` identify the source of a diagnostic.
Failed steps do not advance the clock or partially change CPU or device state. Earlier successful steps remain committed.

**The clock now uses instruction-specific costs, but it is not cycle-accurate.**
Device clocks update in batches after instructions, exception entries, DMA units, or HALT idle intervals.
Reads observe register state before that batch.
Writes, including timer start/stop and IF acknowledgement, take effect before the entire batch.
This does not reproduce bus-access timing within an instruction or IRQ synchronization delays.

`Cpu::step` remains a CPU-only API. It does not honor HALT, execute DMA, advance device clocks, or sample device IRQs.
`Cpu::step_timed` executes the same operation and returns `StepTiming`, without advancing devices.
`Machine::step` uses that result to advance device clocks. Failed steps preserve the previous timing result.
`Memory::advance_cycles(n)` advances device clocks, latches DMA requests, and updates HALT wake-up state.
It does not execute CPU instructions or DMA units.
Multiple unserviced DMA requests coalesce; bulk clock advances do not replay every missed transfer.
Reads and writes alone consume no cycles.
`Memory::cycles()` and `Machine::cycles()` report the supplied cycle total as a wrapping 64-bit counter.

Mapped input/output (I/O) registers:

| Address | Register | Behavior |
| --- | --- | --- |
| `0x04000100` + `4*n` | Timer n counter/reload | Reads the counter; writes the separate reload value |
| `0x04000102` + `4*n` | Timer n control | Prescaler, count-up, local IRQ enable, and start/stop |
| `0x04000200` | IE | Enables selected interrupt sources; bits 0–13 are writable |
| `0x04000202` | IF | Latches requests; writing 1 clears the corresponding bit |
| `0x04000204` | WAITCNT | Configures first/second ROM access wait states; unused bits read as zero |
| `0x04000208` | IME | Bit zero gates IRQ delivery; other bits read as zero |
| `0x04000300` | POSTFLG | Bit-zero post-boot flag; CPU writes require BIOS execution |
| `0x04000301` | HALTCNT | Write-only low-power control; HALT supported, STOP returns a diagnostic |

Timers count at supplied clock rates divided by 1, 64, 256, or 1024.
Timers 1–3 can instead count the preceding timer's overflows. Timer 0 ignores count-up selection.
Starting a stopped timer copies reload into the counter. Overflow also reloads the counter.
Writing reload while running leaves the counter unchanged until overflow or restart.
A combined 32-bit reload/control write uses the new reload value when starting the timer.
Stopping a timer freezes its counter. Restarting it reloads the counter.
Bulk clock advances preserve all overflow pulses through cascaded timers without looping once per cycle.

Overflow latches IF only when the timer's local IRQ-enable bit is set.
IE, IME, and CPSR.I gate delivery, not the pending flag.
Entering IRQ mode does not clear IF. Software must acknowledge the request.
Unused register bits read as zero. Unknown I/O addresses and mirrors remain unmapped.
Byte, halfword, and word accesses are supported for this register subset.

Timer startup delays and shared prescaler phase are not modeled.
The provisional timer model resets its private prescaler phase on enable or clock-source changes.
Audio events, STOP, and interrupt sources other than timers, display events, and DMA completion are not implemented.

`--timer-demo` executes original ARM code that configures Timer 0, IE, and IME.
The timer overflows after 16 supplied cycles and enters the original handler through vector `0x18`.
The handler stops Timer 0, acknowledges IF, increments r10, and returns with `SUBS pc, lr, #4`.
The demo ends after 21 steps and 153 nominal cycles with one handler call, using default WAITCNT settings.
These counts test the timing model; they are not hardware timing measurements.
Its test handler deliberately changes r2 and r10; it does not implement the Nintendo BIOS calling convention.

## HALT and power-control registers

Writing HALTCNT with bit 7 clear requests HALT. Bits 0–6 are ignored.
The current instruction finishes before the machine pauses CPU execution.
Timers, display timing, and DMA continue to run. HALT does not pause the native window or host input.
`Memory::halted()` and `Machine::halted()` report whether HALT is still waiting.

HALT ends when `IE & IF` becomes nonzero, even when IME or CPSR.I blocks IRQ delivery.
An already pending enabled request prevents the CPU from sleeping.
Wake-up does not acknowledge IF. Clearing IF after wake-up does not put the CPU back into HALT.
On the next machine step, ready DMA still takes priority.
The CPU then enters an unmasked IRQ handler or resumes the next instruction.

Each `HaltIdle` step advances to the earliest display edge or independently clocked timer overflow.
Display edges include every HBlank entry and scanline start. Cascaded timers receive pulses at their predecessor's overflow.
These bounds prevent idle advances from skipping DMA triggers or wake-up events.
They also keep frame execution bounded when no interrupt can wake the CPU.
Idle steps neither fetch instructions nor change CPU registers; code, data, and internal cycle counts remain zero.
Exact hardware entry/exit delays and pipeline effects are not modeled.

CPU writes to POSTFLG and HALTCNT take effect only when the executing instruction address is within the 16 KiB BIOS region.
ARM and Thumb use the same rule. This is an instruction-address check, not a simulated prefetched PC signal.
CPU writes from RAM or cartridge ROM are ignored, including STOP requests.
Bare `Memory::write*` calls remain host/debug setup and can write these registers without a CPU access context.
DMA writes to the power-control block return `DmaError::PowerControlDestination`; this hardware edge case is not modeled.

POSTFLG resets to zero and stores only bit 0. Setting it does not perform a BIOS boot or initialize RAM.
HALTCNT reads return zero as a placeholder, not hardware open-bus behavior.
The reserved bytes at `0x04000302..0x04000303` read zero and ignore writes.
Byte, halfword, and word writes are supported; a halfword/word at POSTFLG also writes HALTCNT.

STOP requests from BIOS code or host setup return `MemoryError::UnsupportedStop` before any write commits.
This prevents STOP from silently behaving like HALT while its clock-gating and wake sources are absent.
Multi-byte and block-store diagnostics preserve POSTFLG, HALT state, CPU registers, and the current step's clock.
CPU-only stepping can record a HALT request but does not enforce the pause. Use `Machine::step()` for device integration.

The graphics demo now uses the optional original BIOS replacement's VBlankIntrWait service.
Its IRQ callback acknowledges VBlank and updates the BIOS RAM flag before the service returns.
