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
Startup scheduling delays, channel-resumption costs, CPU fetch-state effects, and display-bus contention remain unmodeled.

Special timing modes (sound FIFO/video capture), Game Pak DRQ, and prohibited source mode 3 return `DmaError::UnsupportedControl`.
DMA reads below work RAM, including BIOS reads, return `DmaError::UnsupportedSource` rather than exposing BIOS bytes.
DMA writes to DMA registers return `DmaError::RegisterDestination`; self-modifying transfers are not supported.
Other accesses use the existing memory map, alignment checks, mirrors, and I/O write rules.
Unmapped memory and cartridge writes remain diagnostics, not hardware open-bus/latch behavior.
A failed unit changes no memory, device state, or clock. Earlier units remain committed; the failed unit remains pending.
Successful units invalidate bounded sequential Thumb IWRAM history because DMA-to-CPU latch ordering is not modeled.
Failed units preserve that history. No DMA value is substituted for a CPU open-bus read.
