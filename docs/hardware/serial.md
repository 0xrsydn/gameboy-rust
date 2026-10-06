# Disconnected serial subset

The core supports disconnected normal-mode transfers, external-clock waiting, idle multiplayer configuration, and general-purpose pins.
Internal clocks shift pulled-high input at nominal GBA rates. No link partner is emulated.
External clock edges, connected multiplayer transfers, UART, Joybus communication, and GPIO interrupts remain unsupported.
Unsupported activity remains diagnostic rather than reporting a fabricated completion.

## Registers

- `SIODATA32` at `0x04000120` holds the 32-bit shift register. Byte lanes and halfword accesses are supported.
- `SIODATA8` at `0x0400012a` holds the 8-bit shift register; its upper byte reads zero.
  Each transfer shifts only its selected data register. The other register retains its value.
- `SIOCNT` at `0x04000128` retains normal 8/32-bit mode, IRQ configuration, clock selection, and the output-data bit.
  The retained mask is `0x508b`. The disconnected serial input is high, except when a general-purpose output drives it low.
  Start bit 7 requests a transfer. Bit 0 selects external or internal clock; bit 1 selects the internal rate.
  Completion or software cancellation clears start. Multiplayer uses the separate format described below; UART remains diagnostic.
- `RCNT` at `0x04000134` retains mode bits, interrupt configuration, output latches, and pin directions with mask `0xc1ff`.
  With bit 15 clear, bits 8 and 14 are writable latches without GPIO interrupt or Joybus effects.
  With bit 15 set, bit 14 selects Joybus; otherwise bit 8 enables GPIO interrupts. Both active configurations remain diagnostic.
  In general-purpose mode, each input reads high through its pull-up. Each output reads its output latch.
  `RCNT=0x8000` therefore reads back as `0x800f`, not `0x8000`.
  Normal and multiplayer low-byte pin readback remains unmapped rather than guessed.
- `JOYCNT` permits clearing already-empty status flags. Joybus interrupt enable is unsupported.
- `JOY_RECV` and `JOY_TRANS` permit only zero writes and zero readback for reset initialization.
  Nonzero writes return an explicit error. Joybus status and data-transfer side effects are not implemented.

GPIO means general-purpose input/output. UART means universal asynchronous receiver/transmitter.
No external serial device is connected. Only an enabled internal serial clock can shift data or generate a completion IRQ.
Configuration that hardware accepts in other modes can still be rejected by this intentionally limited subset.

## External-clock waiting

In normal 8-bit or 32-bit mode, setting start with clock-source bit 0 clear requests externally clocked communication.
No device is connected, so no external clock edges arrive. Start remains set; data does not shift and no completion IRQ occurs.
The internal-rate selector, bit 1, does not supply clocks when bit 0 is clear.
CPU cycles, timer overflows, display events, and DMA activity cannot manufacture external serial clocks.
Repeated start writes do not complete the request. Software can cancel it by clearing start, including through BIOS serial reset.

General-purpose mode does not shift serial data. A retained external-clock start bit has no link-port effect there.
Returning to normal mode still cannot complete a transfer without external clock edges.
Selecting internal clock while an unshifted request is pending starts a fresh nominal bit period.
This is a disconnected waiting model, not an implementation of a connected link or an external clock-input API.

Original tests cover both widths, rate settings, control lanes, cancellation, GPIO selection, HALT/STOP, and ARM/Thumb stores.
DMA tests distinguish its own completion IRQ from a serial IRQ. Invalid word/block stores cannot leave a partial start request.

## Internal clocks and completion

Normal mode shifts most-significant bit first. At each nominal sampling edge, the selected register shifts left and receives a one.
The disconnected SI input is high. Completion therefore leaves `0xff` or `0xffffffff`, without inventing peer data.
Reads between edges expose the partially shifted value. SO waveforms and normal-mode RCNT pin samples remain unmodeled.

At the nominal 16,777,216 Hz system clock:

| SIOCNT bit 1 | Cycles per bit | 8-bit transfer | 32-bit transfer |
| --- | --- | --- | --- |
| 0 | 64 | 512 cycles | 2,048 cycles |
| 1 | 8 | 64 cycles | 256 cycles |

The first sample occurs one full bit period after activation. Selecting internal clock for an unshifted external request uses this same rule.
A repeated start write with unchanged width/clock fields preserves the current phase and remaining bit count.
A width/clock change before any bit has shifted starts a fresh period. Data writes before that first bit preserve the phase.
Software cancellation clears progress but retains already shifted data.
Whole-access writes merge control and data lanes before applying start/cancel behavior; byte writes remain separate accesses.

Completion clears start and latches IF bit 7 when SIOCNT bit 14 is enabled at the final edge.
IE, IME, and CPSR.I gate wake-up or delivery, not request latching. Clearing IF acknowledges the request.
A completed request does not reassert IF without another transfer.
HALT continues internal shifting and bounds its idle batch at serial completion. STOP freezes the bit phase and remaining count.
Only keypad wake is implemented for STOP; serial GPIO wake still requires its missing external-input model.

### Bus timing and validation

Serial state shares the timer/audio transaction during CPU instructions, IRQ entry, and DMA units.
Each access observes all elapsed source/data cycles through its bus-completion phase.
A start write excludes its preceding cycles. A load excludes its trailing internal cycle.
IF reads and acknowledgements include serial completion through that access, alongside timer requests.
Successful steps commit serial state and requests once. Failed steps discard speculative shifts, writes, and completion IRQs.
The display/capture scheduler does not clock serial again after that commit.
CPU-only stepping can change registers but does not advance serial clocks.

Block-store preflight simulates ordered access phases on temporary timing/device copies before committing any write.
Validation can therefore accept reconfiguration when an earlier request completes before the write reaches the bus.
The actual transfers then execute once from the original state. Other IF sources and DMA completion flags remain independent.

### Bounded behavior

After any bit has shifted, changes to width, clock selection/rate, or the selected data register remain diagnostic while start stays set.
Selecting GPIO during an internally clocked request also remains diagnostic. Clear start before these operations.
IRQ-enable and idle SO configuration writes remain supported without restarting progress.
These limits also apply to BIOS services that write serial registers; cancel active transfers before requesting serial reset.
Unverified live reconfiguration is not silently approximated.

The period arithmetic follows documented GBA rates. Activation phase, mid-transfer register visibility, and exact pin edges lack physical-hardware validation.
No external pin waveform, cable, connected partner, or serial-device protocol is implemented.
Original tests cover individual shifts, batching, cancellation, IRQ gating, HALT/STOP, bus phases, DMA, capture independence, and rollback.

## Disconnected multiplayer configuration

With RCNT bit 15 clear, SIOCNT mode bits 12–13 equal to `2` select multiplayer format.
The retained control mask is `0x6f03`: baud selection, unused writable bits 8–11, mode, and IRQ enable.
This subset models an idle unit without a cable, not a parent with missing players.

- SI reads high in status bit 2, identifying a child rather than a parent.
- SD reads high in bit 3 because the local idle output is high. This does not establish connected peers.
- ID bits 4–5 read a deterministic zero placeholder. Hardware does not define the ID before a completed transfer.
  Zero must not be interpreted as an assigned parent role; SI determines that role.
- Error bit 6 and busy bit 7 remain clear. No parent supplies the clock that starts a child transfer.
- Start is read-only for a child. Writing it cannot start a transfer, reset receive data, or request an IRQ.

All baud settings retain this idle behavior. Elapsed cycles, DMA, and repeated start writes cannot create a linked transfer.
No multiplayer event can wake HALT. STOP retains the configuration without advancing clocks.
When GPIO is selected, SI/SD status follows the existing GPIO pin model; multiplayer transfers still cannot run.
RCNT low-byte samples remain unmapped outside GPIO mode. Floating clock-pin timing is not guessed.

### Multiplayer data registers

`SIOMLT_SEND` at `0x0400012a` is a 16-bit latch in multiplayer format.
It shares its low byte with normal-mode SIODATA8. Normal upper-byte reads return zero and writes are ignored.
Normal 8-bit shifts change only the low byte. The hidden upper byte survives mode changes in this bounded model.
A word write to SIOCNT selects the final format before applying both SEND lanes.

`SIOMULTI0` through `SIOMULTI3` occupy halfwords at `0x04000120`, `0x04000122`, `0x04000124`, and `0x04000126`.
The first two alias SIODATA32. The last two are accessible only with multiplayer format selected, including under GPIO.
Byte, halfword, and word accesses preserve independent lanes.
Receive latches start at zero under the synthetic reset policy and retain data across mode changes.
Selecting multiplayer or writing child start does not replace them with `0xffff`, local SEND data, or invented peer data.

Receive writes follow GBATEK's R/W register description. The pinned mGBA implementation instead leaves these writes unhandled.
Physical receive-write behavior, reset values, and hidden upper-byte retention have not been independently measured.
These are bounded register-model choices, not evidence of successful reception or full firmware-reset accuracy.

### Validation and limits

Mode changes use staged serial state at the bus-completion phase.
A high-byte mode write cannot silently cancel a busy normal transfer through multiplayer's read-only start mask.
Clear start explicitly, or wait for normal completion. A completion IRQ already requested remains pending after mode selection.
Block-store preflight observes earlier mode writes and rejects later invalid accesses without committing preceding writes.

Original tests cover control masks, all baud selections, register lanes, aliases, GPIO, HALT/STOP, and ARM/Thumb/DMA stores.
Bus-phase tests cover normal completion, retryable diagnostics, block loads/stores, and mode-dependent staged reads.
There is no parent, cable topology, external clock input, multiplayer timeout, or linked-game compatibility claim.

## BIOS reset

RegisterRamReset always clears the low halfword of SIODATA32, even without bit 5.
The upper halfword remains unchanged in this functional subset.
With bit 5 selected, the firmware clears SIOCNT and SIODATA8, selects general-purpose inputs, and clears the supported Joybus reset registers.
It uses ordinary ARM bus stores, not a host-side reset bypass.
Clearing SIOCNT cancels a pending external-clock request. Unselected serial reset leaves that request pending.
The firmware writes SIODATA8 after selecting normal format. This does not clear the retained upper multiplayer SEND byte.
Full multiplayer reset side effects remain unverified; this increment does not extend the firmware reset sequence.

Original tests cover independent byte lanes, input pull-ups, output directions, reset flag selection, and explicit unsupported operations.
Whole-access and block-store validation prevent a rejected control value from committing preceding bytes or registers.
Static RCNT mode rejection uses the complete high byte. Live-transfer validation additionally uses staged serial state.
Mode-gating tests cover every high-byte value, previous modes, low-byte preservation, CPU stores, DMA retries, and unchanged interrupt state.
Inactive configuration bits do not wake HALT. RCNT word accesses still reject the unmapped padding at `0x04000136`.

## References

- [GBATEK reset functions](https://problemkaputt.de/gbatek-bios-reset-functions.htm): reset flags, general-purpose selection, and the unconditional SIODATA32 side effect.
- [GBATEK normal serial mode](https://problemkaputt.de/gbatek-sio-normal-mode.htm): data widths, MSB-first shifting, rates, pulled-high SI, start/completion, IRQ selection, and external-clock waiting.
- [GBATEK multiplayer mode](https://problemkaputt.de/gbatek-sio-multi-player-mode.htm): child start restrictions, SI/SD status, undefined pre-transfer ID, register widths, and R/W receive registers.
- [LinkRawCable at c61bf351](https://github.com/afska/gba-link-connection/blob/c61bf351f68ad2d6e1c9d72d70e21bec19adfc0b/lib/LinkRawCable.hpp): public API usage for mode selection, SEND, receive words, and separate role/readiness/ID checks. Its disconnected-cable warning is not a pin-level measurement.
- [GBATEK mode summary](https://problemkaputt.de/gbatek-sio-control-registers-summary.htm): RCNT/SIOCNT mode selection.
- [mGBA serial implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/sio.c): cross-checked normal transfer-duration arithmetic and completion IRQ handling. Its no-driver scheduled completion and zero receive data are not copied; they do not establish disconnected hardware behavior. Multiplayer comparison covers SI/SD defaults, receive-write disagreement, and uncertain floating SC timing.
- [GBATEK](https://mgba-emu.github.io/gbatek/): GBA general-purpose pin directions, internal pull-ups, SI falling-edge interrupts, and the SIO mode-selection table.
- [mGBA BIOS implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional serial reset defaults, not copied source.

Exact firmware write ordering and hardware pin timing remain unverified.
