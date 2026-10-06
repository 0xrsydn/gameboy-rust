# Timers, interrupts, HALT, and STOP

## Timers, interrupt registers, and the machine clock

`Machine::new(cpu, memory)` combines the CPU and memory bus.
`Machine::step()` first checks STOP and returns `StopIdle` with zero timing if the system is still stopped.
Otherwise it services one ready DMA data unit, keeping the CPU paused.
If HALT is waiting and no DMA is ready, it advances device clocks to the next event without executing CPU code.
Otherwise, it samples the IRQ line, including the CPU's interrupt mask, then enters IRQ mode or executes one instruction.
Each successful step advances display and timer clocks by its nominal cost.
`Machine::last_timing()` reports separate code, data, internal, and idle cycle totals for the last successful step.
`StepTiming::idle_cycles` counts device-clock cycles during HALT; it does not count CPU work.
IRQ entry is a separate step; the vector instruction runs on the following step.
A device event during a step can trigger IRQ entry once no DMA is ready and CPU masks allow delivery.
Bus writes take effect before the step's device-clock update.
`StepKind::Dma { channel }` identifies a DMA unit. `StepKind::HaltIdle` identifies one bounded HALT clock advance.
`StepKind::StopIdle` identifies a stopped system with no clock progress.
The other step kinds remain `Instruction` and `IrqEntry`.
`MachineError::Cpu` and `MachineError::Dma` identify the source of a diagnostic.
Failed steps do not advance the clock or partially change CPU or device state. Earlier successful steps remain committed.

**The clock now uses instruction-specific costs, but it is not cycle-accurate.**
Device clocks update in batches after instructions, exception entries, DMA units, or HALT idle intervals.
Reads observe register state before that batch.
Writes, including timer start/stop and IF acknowledgement, take effect before the entire batch.
This does not reproduce bus-access timing within an instruction or IRQ synchronization delays.

`Cpu::step` remains a CPU-only API. It does not honor HALT/STOP, execute DMA, advance device clocks, or sample device IRQs.
`Cpu::step_timed` executes the same operation and returns `StepTiming`, without advancing devices.
`Machine::step` uses that result to advance device clocks. Failed steps preserve the previous timing result.
`Memory::advance_cycles(n)` advances device clocks, latches DMA requests, and updates HALT wake-up state while not stopped.
In STOP, it ignores supplied cycles and leaves device phases and captured frames unchanged.
It does not execute CPU instructions or DMA units.
Multiple unserviced DMA requests coalesce; bulk clock advances do not replay every missed transfer.
Reads and writes alone consume no cycles.
`Memory::cycles()` and `Machine::cycles()` report elapsed emulated clocks as a wrapping 64-bit counter, excluding stopped time.

Mapped input/output (I/O) registers:

| Address | Register | Behavior |
| --- | --- | --- |
| `0x04000100` + `4*n` | Timer n counter/reload | Reads the counter; writes the separate reload value |
| `0x04000102` + `4*n` | Timer n control | Prescaler, count-up, local IRQ enable, and start/stop |
| `0x04000130` | KEYINPUT | Ten active-low button bits; read-only |
| `0x04000132` | KEYCNT | Button selection, keypad IRQ enable, and OR/AND condition |
| `0x04000200` | IE | Enables selected interrupt sources; bits 0–13 are writable |
| `0x04000202` | IF | Latches requests; writing 1 clears the corresponding bit |
| `0x04000204` | WAITCNT | Configures first/second ROM access wait states; unused bits read as zero |
| `0x04000208` | IME | Bit zero gates IRQ delivery; other bits read as zero |
| `0x04000300` | POSTFLG | Bit-zero post-boot flag; CPU writes require BIOS execution |
| `0x04000301` | HALTCNT | Write-only low-power control; HALT and keypad-wake STOP supported |

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
Audio events and interrupt sources other than timers, display events, DMA completion, and keypad input are not implemented.

`--timer-demo` executes original ARM code that configures Timer 0, IE, and IME.
The timer overflows after 16 supplied cycles and enters the original handler through vector `0x18`.
The handler stops Timer 0, acknowledges IF, increments r10, and returns with `SUBS pc, lr, #4`.
The demo ends after 21 steps and 151 nominal cycles with one handler call, using default WAITCNT settings.
Its initial ARM ROM fetch uses PC+8 sequential timing; cold pipeline filling has no separate startup charge.
These counts test the timing model; they are not hardware timing measurements.
Its test handler deliberately changes r2 and r10; it does not implement the Nintendo BIOS calling convention.

## Keypad interrupt control

`KEYCNT` resets to zero and stores only bits selected by `0xc3ff`:

| Bits | Meaning |
| --- | --- |
| 0–9 | Select A, B, Select, Start, Right, Left, Up, Down, R, and L |
| 10–13 | Unused; read as zero |
| 14 | Enable keypad interrupt requests |
| 15 | Zero: OR condition; one: AND condition |

OR matches when any selected button is pressed.
AND matches when every selected button is pressed; unselected pressed buttons do not prevent a match.
An empty selection never matches OR and always matches AND.

A matching enabled sample can latch IF bit 12, independently of IE, IME, and CPSR.I.
Changing input or disabling KEYCNT does not clear an existing request.
Software acknowledges IF by writing bit 12 as one.
IE bit 12 gates HALT wake-up. IME and CPSR.I gate IRQ delivery, not wake-up or request latching.
Ready DMA units still take priority over CPU IRQ entry.
The original BIOS IntrWait can consume keypad events through the normal IRQ callback contract.

### Sampling policy and limits

While the system clock runs, the functional model follows mGBA's polling behavior, not a verified hardware edge detector:

- Each `Memory::set_buttons` call samples the input, including repeated identical values.
- Each successful write touching KEYCNT samples after the complete register update.
- OR requests on every matching enabled sample, so another identical input sample can reassert an acknowledged request.
- AND remembers the complete pressed-button snapshot and suppresses an identical matching sample.
- A different matching snapshot requests again, including changes to unselected buttons.
- An enabled nonmatching sample clears that history. Disabled samples retain it.
- Newly selected buttons are removed from the remembered snapshot before testing a control write.
  A newly selected held button can therefore cause another AND request.
- Reads, KEYINPUT-only writes, IF acknowledgement, and clock advancement do not sample the keypad.

This policy makes IRQ generation depend on input sampling, not only on elapsed emulated time.
Host input calls consume no emulated cycles and do not modify CPU registers.
They can wake HALT immediately; `Machine::step` subsequently samples the IRQ line.
The desktop demos currently supply frame-based input snapshots.
Physical key-event timing, switch bounce, exact hardware retrigger rules, and synchronization delays remain unverified.
STOP uses a separate live-condition wake path described below, without latching IF or updating this polling history.

Byte writes merge with the untouched KEYCNT byte.
Halfword writes, and word writes at KEYINPUT, evaluate the final KEYCNT value once.
They do not generate requests from temporary byte-by-byte values.
The lower half of a word write remains a read-only KEYINPUT write.
CPU, DMA, and host/debug writes use the same path.
Failed writes and failed block stores leave control, IRQ history, pending flags, and HALT state unchanged.

The current BIOS SoftReset and RegisterRamReset subset preserve KEYCNT and button state.
RegisterRamReset bit 7 still acknowledges IF, but a later matching input sample can request keypad IRQ again.

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

Multi-byte and block-store diagnostics preserve POSTFLG, power state, CPU registers, and the current step's clock.
CPU-only stepping can record HALT/STOP but does not enforce either pause. Use `Machine::step()` for device integration.

## STOP with keypad wake-up

Writing HALTCNT with bit 7 set requests STOP. Bits 0–6 are ignored.
The instruction that writes STOP finishes and pays its full nominal cycle cost.
Later machine steps perform no CPU fetch, DMA transfer, timer tick, or display update.
`StepKind::StopIdle` reports zero timing; it does not represent elapsed host wall time.
`Memory::stopped()` and `Machine::stopped()` report this state. The corresponding `halted()` methods return false in STOP.

A matching live keypad condition wakes STOP when KEYCNT bit 14 and IE bit 12 are enabled.
The same selected-button OR/AND logic applies, including the empty-mask behavior.
IME and CPSR.I do not gate wake-up in this functional model.
A matching condition already present at entry prevents a sustained stop.
Stale IF flags alone cannot wake STOP, including an old keypad request.
Timer, display, and DMA requests cannot wake STOP.

An input sample supplied while stopped updates KEYINPUT and the wake condition without setting IF.
It does not consume the normal running-state keypad polling history.
This follows GBATEK's note that IF is not set while the system clock is stopped.
Existing IF bits remain unchanged. After wake-up, an old pending enabled IRQ may still be delivered.
A later running-state input sample follows the normal keypad IRQ policy.
Host/debug writes to IE or KEYCNT can also satisfy the wake condition; stopped CPU code cannot execute such writes.

After wake-up, ready DMA again takes priority, then IRQ delivery or CPU execution resumes.
Timer prescaler phases, display position, RAM, and completed captured frames are retained, not reset.
STOP does not automatically set forced blank or clear DISPCNT.
Software can force blank before STOP if it wants the displayed image disabled.

`Machine::run_until_vblank` returns `FrameRunError::Stopped` on a stopped idle step rather than exhausting the step limit.
A zero step limit still returns `StepLimit(0)`.
The demo frame runner maps this to `GraphicsError::Stopped`, including during startup, without changing its output image.
A host can supply input and retry after wake-up.
The current desktop demos do not enter STOP; the frontend does not yet provide a general game-sleep screen.

Only keypad wake-up is implemented. General-purpose serial and Game Pak wake sources require their missing external devices.
Oscillator restart delays, exact entry/wake edges, and hardware-specific IF behavior remain unverified.
There is no host-thread sleep, wall-clock accounting, or audio clock implementation.

The graphics demo now uses the optional original BIOS replacement's VBlankIntrWait service.
Its IRQ callback acknowledges VBlank and updates the BIOS RAM flag before the service returns.
