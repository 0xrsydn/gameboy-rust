# Disconnected serial subset

The core supports normal-mode data registers, disconnected external-clock waiting, and general-purpose pins.
It does not support internally clocked transfers, external clock edges, multiplayer, UART, Joybus communication, or external serial interrupts.
Unsupported activity remains diagnostic rather than reporting a fabricated completion.

## Registers

- `SIODATA32` at `0x04000120` is a four-byte latch in the supported normal-mode subset.
- `SIODATA8` at `0x0400012a` retains its low byte; its upper byte reads zero.
- `SIOCNT` at `0x04000128` retains normal 8/32-bit mode, IRQ configuration, clock selection, and the output-data bit.
  The retained mask is `0x508b`. The disconnected serial input is high, except when a general-purpose output drives it low.
  Start bit 7 can latch when clock-source bit 0 is clear. Software clears bit 7 to cancel the request.
  Setting both bits 7 and 0 returns an internally-clocked-transfer diagnostic. Multiplayer/UART mode bit 13 remains unsupported.
- `RCNT` at `0x04000134` retains mode bits, interrupt configuration, output latches, and pin directions with mask `0xc1ff`.
  With bit 15 clear, bits 8 and 14 are writable latches without GPIO interrupt or Joybus effects.
  With bit 15 set, bit 14 selects Joybus; otherwise bit 8 enables GPIO interrupts. Both active configurations remain diagnostic.
  In general-purpose mode, each input reads high through its pull-up. Each output reads its output latch.
  `RCNT=0x8000` therefore reads back as `0x800f`, not `0x8000`.
  Normal-mode low-byte pin readback remains unmapped rather than guessed.
- `JOYCNT` permits clearing already-empty status flags. Joybus interrupt enable is unsupported.
- `JOY_RECV` and `JOY_TRANS` permit only zero writes and zero readback for reset initialization.
  Nonzero writes return an explicit error. Joybus status and data-transfer side effects are not implemented.

GPIO means general-purpose input/output. UART means universal asynchronous receiver/transmitter.
No external serial device is connected. Elapsed cycles alone do not change serial data or generate an IRQ.
Configuration that hardware accepts in other modes can still be rejected by this intentionally limited subset.

## External-clock waiting

In normal 8-bit or 32-bit mode, setting start with clock-source bit 0 clear requests externally clocked communication.
No device is connected, so no external clock edges arrive. Start remains set; data does not shift and no completion IRQ occurs.
The internal-rate selector, bit 1, does not supply clocks when bit 0 is clear.
CPU cycles, timer overflows, display events, and DMA activity cannot manufacture external serial clocks.
Repeated start writes do not complete the request. Software can cancel it by clearing start, including through BIOS serial reset.

General-purpose mode does not shift serial data. A retained external-clock start bit has no link-port effect there.
Returning to normal mode still cannot complete a transfer without external clock edges.
Selecting internal clock while start is set remains diagnostic, including after an external request.
This is a disconnected waiting model, not an implementation of a connected link or an external clock-input API.

Original tests cover both widths, rate settings, control lanes, cancellation, GPIO selection, HALT/STOP, and ARM/Thumb stores.
DMA tests distinguish its own completion IRQ from a serial IRQ. Invalid word/block stores cannot leave a partial start request.

## BIOS reset

RegisterRamReset always clears the low halfword of SIODATA32, even without bit 5.
The upper halfword remains unchanged in this functional subset.
With bit 5 selected, the firmware clears SIOCNT and SIODATA8, selects general-purpose inputs, and clears the supported Joybus reset registers.
It uses ordinary ARM bus stores, not a host-side reset bypass.
Clearing SIOCNT cancels a pending external-clock request. Unselected serial reset leaves that request pending.

Original tests cover independent byte lanes, input pull-ups, output directions, reset flag selection, and explicit unsupported operations.
Whole-access and block-store validation prevent a rejected control value from committing preceding bytes or registers.
RCNT selection bits share the high byte. Each high-byte write replaces the complete selection, so validation needs no retained-mode shadow.
Mode-gating tests cover every high-byte value, previous modes, low-byte preservation, CPU stores, DMA retries, and unchanged interrupt state.
Inactive configuration bits do not wake HALT. RCNT word accesses still reject the unmapped padding at `0x04000136`.

## References

- [GBATEK reset functions](https://problemkaputt.de/gbatek-bios-reset-functions.htm): reset flags, general-purpose selection, and the unconditional SIODATA32 side effect.
- [GBATEK normal serial mode](https://problemkaputt.de/gbatek-sio-normal-mode.htm): data widths, input status, writable unused RCNT bits 8/14, and external-clock slave start/wait/timeout behavior.
- [mGBA serial implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/sio.c): compared normal-mode control and completion paths. Its no-driver scheduled completion does not establish disconnected external-clock hardware behavior; this core does not copy that fallback.
- [GBATEK](https://mgba-emu.github.io/gbatek/): GBA general-purpose pin directions, internal pull-ups, SI falling-edge interrupts, and the SIO mode-selection table.
- [mGBA BIOS implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional serial reset defaults, not copied source.

Exact firmware write ordering and hardware pin timing remain unverified.
