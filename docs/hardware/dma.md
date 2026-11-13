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

- Immediate: ready at the first step boundary at least 3 cycles after the enabling CPU instruction began, as mGBA
  schedules it. A store taking 3 or more cycles (from ROM, say) lets the transfer start right after it; a 2-cycle
  store from internal RAM lets the next instruction run first. Host and DMA writes start it at once. HALT idling stops
  at the pending start. Repeat does not keep it running. This raised the mGBA suite's timing results from 1792 to 1824
  without changing any memory or DMA result; mGBA's own cost attribution (charging a store's following fetch to the
  next instruction) lowered them, so cycles count with this emulator's attribution.
- VBlank and HBlank transfers wait the same 3 cycles after their display edge (as mGBA schedules both): the transfer is
  ready at the first step boundary at least 3 cycles after line 160 begins or after cycle 1,006 of a visible line.
  A clock batch that ends past the edge has already spent part of the wait; HALT idles through the rest. The mGBA
  suite's "DMA Prefetch" test passes with a wait of 1 to 4 cycles and fails with 0 or with 5 or more, so the value
  inside that range is mGBA's, not a measurement.
- VBlank: requested on entry to line 160.
- HBlank: requested at HBlank entry on visible lines 0–159 only.

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
When source and destination are both Game Pak addresses, the first unit's write is sequential too: the bus is already
addressing the cartridge. The [mGBA test suite](../public-mgba-suite.md)'s ROM-to-ROM cases measure N+S for the first
halfword unit (19 cycles over calibration with default waits, not 21).
Each step reports zero code cycles. Overlapping transfers read each unit after the preceding write; they are not bulk host copies.
Timers stage progress through startup, source, and destination phases. Timer reads sample after source completion; writes apply after destination completion.
Timer IF acknowledgements use that same ordering. Success commits the staged bank; failure discards it.
Other devices then advance in bulk without ticking timers twice. Exact physical DMA startup timing remains unverified.
A successful unit breaks the next nominal CPU code-access sequence, even if DMA accessed only RAM.
The resumed instruction requests N once; enabled opcode prefetch can still satisfy a matching request.
Opcode prefetch pauses while DMA owns the bus, and the queue it had built is still there when the CPU resumes.
This holds whether the transfer touches only RAM or the cartridge. A halfword that was in flight continues from the
same phase afterwards. One stall cycle is added when the transfer's first unit reaches the cartridge during the last
cycle of that halfword: one cycle left for a cartridge source, two cycles left for a RAM source with a cartridge
destination (the RAM read comes first). DMA never consumes queued opcodes.
Only successful units commit; WAITCNT destinations apply configuration changes after the transfer.

These rules are fitted to the suite's sixteen hardware-verified DMA timing tests (1 and 16 units, 16- and 32-bit,
all four RAM/ROM source and destination pairs), each run from ARM and Thumb code with prefetch on and off under four
wait settings. All of them pass. The measured halfword phases are tied to the wait setting in those tests, so the
stall rule's phase condition is the simplest reading of the data, not an independently isolated fact.
Startup placement inside the interrupted instruction is still nominal.
See [CPU resume timing](cpu.md#nominal-cpu-resume-after-dma) for failure, idle, and refill rules.
Channel-resumption costs, a full CPU fetch pipeline, and display-bus contention remain unmodeled.

Special timing supports DMA1/FIFO A and DMA2/FIFO B with four-word blocks, forced word width, and fixed destinations.
Timer-driven requests, repeat, and completion IRQs are described in [Direct Sound](audio.md).
Other special-mode pairings, video capture, Game Pak DRQ, and prohibited source mode 3 return `DmaError::UnsupportedControl`
when the channel would transfer, not when it is programmed: the mGBA suite writes and restores every control bit.
DMA0 special timing never triggers, as in mGBA. DMA3 special timing (video capture) is reported at its first visible HBlank.
DMA reads below work RAM reuse known channel data as described below. They never expose BIOS bytes.
DMA writes to DMA registers return `DmaError::RegisterDestination`; self-modifying transfers are not supported.
Writes into the mapped 16 KiB BIOS [complete without changing its image](bios.md#writes-to-the-mapped-bios).
Source reads, channel data, address progression, timing, and completion IRQs still apply to these ignored writes.
DMA0–2 mask destination bit 27, so a programmed cartridge address can instead select the BIOS region.
Other accesses use the existing memory map, alignment checks, mirrors, and I/O write rules.
Other unmapped reads remain diagnostics, not general DMA open-bus behavior. Writes to the `0x08`–`0x0c` ROM windows, to unused memory, and to unused I/O addresses are ignored.
A DMA0–2 destination aimed at the cartridge often masks into unused memory this way, as the mGBA suite's ROM store test expects; the `0x0d` window (unless EEPROM is selected) and `0x04000800` remain diagnostics.
A failed unit changes no memory, device state, or clock. Earlier units remain committed; the failed unit remains pending.
Successful units update local IWRAM lanes for their actual IWRAM reads and writes, in that order.
No CPU continuation is required; DMA can establish lanes before the first CPU instruction.
Accesses elsewhere leave those local lanes unchanged. Failed units preserve history.
A resumed Thumb PC+4 fetch updates its addressed halfword; a cold pipeline fill drives all its new samples.
Thumb code running from IWRAM never takes a DMA value in place of those lanes.

## Bus value after a transfer

The first instruction that runs after a DMA unit reads the unit's data, not its own prefetched opcode, from unused
memory and from write-only I/O registers. A halfword unit leaves its halfword on both halves. The following
instruction reads its own fetch again, and an IRQ entry in between discards the value. This is mGBA's rule (the
instruction one opcode past the transfer reads `gba->bus`), and the mGBA suite's "DMA Prefetch" test checks it for
Thumb code in ROM. Thumb code in IWRAM is excluded because its lane history above already records what the unit drove.
On hardware the transfer takes the bus between that instruction's opcode fetch and its data access; this emulator
runs the whole unit before the instruction, so only the value, not the sub-instruction order, is modeled.
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
The known channel value supplies a CPU open-bus read only as described under [Bus value after a transfer](#bus-value-after-a-transfer).
