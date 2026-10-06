# Direct memory access (DMA)

Four DMA channels occupy `0x040000b0..0x040000df`. Each channel has a 12-byte register block:

| Offset | Register | Behavior |
| --- | --- | --- |
| `+0` | SAD | Write-only initial source address |
| `+4` | DAD | Write-only initial destination address |
| `+8` | CNT_L | Write-only halfword/word count |
| `+10` | CNT_H | Read/write address controls, repeat, width, timing, IRQ, and enable |

Byte, halfword, and word register writes are supported. A combined count/control write sets the count before enabling DMA.
Control masks are `0xf7e0` for DMA0–2 and `0xffe0` for DMA3.
Source addresses use 27 bits on DMA0 and 28 bits on DMA1–3.
Destination addresses use 27 bits on DMA0–2 and 28 bits on DMA3.
Initial addresses align down to the selected halfword or word width.
DMA0–2 use 14-bit counts; DMA3 uses 16-bit counts. Zero means 16,384 or 65,536 data units, respectively.
Write-only register reads return zero as a placeholder, not hardware open-bus behavior.

A rising enable bit copies the programmed addresses and count into internal transfer state.
Register writes alone do not move data. `Machine::step()` services one halfword or word at a time.
Source modes are increment, decrement, or fixed. Destination modes also include increment/reload.
Game Pak source addresses increment regardless of the selected source mode.
Writes to programmed addresses/count do not alter an active block's internal pointers or remaining count.
Disable the channel before changing its width, address modes, or start timing; live reconfiguration is not hardware-validated.

Supported triggers:

- Immediate: ready after the enabling instruction; repeat does not keep it running.
- VBlank: ready on entry to line 160.
- HBlank: ready at HBlank entry on visible lines 0–159 only.

Blanking triggers do not depend on DISPSTAT IRQ enables or forced blank.
Enabling during an existing blank period waits for the next edge.
A busy channel ignores additional edges. Pending requests are not a queue.
On repeated blanking transfers, completion reloads the programmed count and optionally the destination.
The source continues from its current position. Non-repeated completion clears the enable bit.
Clearing enable manually cancels a waiting or active transfer.

DMA0 has highest priority, followed by DMA1, DMA2, and DMA3.
A higher-priority request can interrupt a lower-priority transfer between data units.
While the system clock runs, the CPU remains paused while any channel is ready; display and timer clocks continue.
STOP freezes DMA before transfer selection, including diagnostics, and retains pending transfer state until wake-up.
Completion with local IRQ enable latches IF bits 8–11, independently of IE, IME, and CPSR.I.
CPU delivery waits until no DMA is ready. IRQ entry does not acknowledge IF.

Timing uses independent source and destination bus costs, including WAITCNT and 128 KiB ROM boundaries.
The first unit uses non-sequential accesses plus two internal cycles. Later units use sequential accesses.
Each step reports zero code cycles. Overlapping transfers read each unit after the preceding write; they are not bulk host copies.
Timers stage progress through startup, source, and destination phases. Timer reads sample after source completion; writes apply after destination completion.
Timer IF acknowledgements use that same ordering. Success commits the staged bank; failure discards it.
Other devices then advance in bulk without ticking timers twice. Exact physical DMA startup timing remains unverified.
A successful unit breaks the next nominal CPU code-access sequence, even if DMA accessed only RAM.
The resumed instruction requests N once; enabled opcode prefetch can still satisfy a matching request.
RAM accesses and the nominal startup cycles advance an active prefetch stream; cartridge accesses cancel it.
Cancellation can add one data cycle at the final halfword phase. DMA never consumes queued opcodes.
Only successful units commit queue progress; WAITCNT destinations apply configuration changes after the transfer.
Exact startup/completion placement remains nominal, not independently verified prefetch arbitration.
See [CPU resume timing](cpu.md#nominal-cpu-resume-after-dma) for failure, idle, and refill rules.
Startup scheduling delays, channel-resumption costs, a full CPU fetch pipeline, and display-bus contention remain unmodeled.

Special timing modes (sound FIFO/video capture), Game Pak DRQ, and prohibited source mode 3 return `DmaError::UnsupportedControl`.
DMA reads below work RAM reuse known channel data as described below. They never expose BIOS bytes.
DMA writes to DMA registers return `DmaError::RegisterDestination`; self-modifying transfers are not supported.
Other accesses use the existing memory map, alignment checks, mirrors, and I/O write rules.
Other unmapped reads and cartridge writes remain diagnostics, not general DMA open-bus behavior.
A failed unit changes no memory, device state, or clock. Earlier units remain committed; the failed unit remains pending.
Successful units update local IWRAM lanes for their actual IWRAM reads and writes, in that order.
No CPU continuation is required; DMA can establish lanes before the first CPU instruction.
Accesses elsewhere leave those local lanes unchanged. Failed units preserve history.
A resumed Thumb PC+4 fetch updates its addressed halfword; a cold pipeline fill drives all its new samples.
No universal DMA-value override is used.
See [local IWRAM DMA history](cpu.md#dma-effects-on-thumb-iwram-continuations) for the scope.

## Retained channel data

Each channel has a separate retained 32-bit data value, distinct from its programmed addresses and count.
A successful mapped word transfer replaces the full value.
A successful mapped halfword transfer duplicates the source halfword into both lanes of the retained word.
Normal source alignment and address masks apply before the read.

A source below `0x02000000` cannot read the bus, including when a BIOS image is present.
With known channel data, a word transfer writes the retained word.
A halfword transfer writes its low half at a word-aligned destination, or its high half when destination bit 1 is set.
The blocked read does not change either retained lane. Source bit 1 does not choose the written lane.
This also applies when a source counter crosses into the blocked region during a block.

Channel data survives completion, disable/cancel, re-enable, and repeated blanking transfers.
Other channels, CPU reads, and host inspection/setup do not change it.
Synthetic startup leaves each channel unknown, not zero.
A blocked source with unknown channel data returns `DmaError::UnsupportedSource` and leaves the unit pending.
A transferred zero is known data and can be reused.

The emulator commits retained data only after a complete successful unit, under its existing atomic diagnostic policy.
Failed source reads, unsupported controls, and failed destinations preserve the previous value and clocks.
Earlier successful units remain committed. Normal destination validation, nominal timing, and completion IRQ rules still apply.
This error policy is not a model of hardware data aborts.

See [the latch evidence and scope](../research/dma-data-latches.md).
General DMA open-bus reads and sub-instruction bus ownership remain unimplemented.
In particular, the known channel value does not supply unused/write-only I/O reads or CPU open-bus snapshots.
