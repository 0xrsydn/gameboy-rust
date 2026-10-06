# Disconnected serial initialization subset

The core supports idle normal-mode data registers and disconnected general-purpose pins.
It does not support link transfers, multiplayer, UART, Joybus communication, or external serial interrupts.
Unsupported activity remains diagnostic rather than reporting a fabricated completion.

## Registers

- `SIODATA32` at `0x04000120` is a four-byte latch in the supported normal-mode subset.
- `SIODATA8` at `0x0400012a` retains its low byte; its upper byte reads zero.
- `SIOCNT` at `0x04000128` retains normal 8/32-bit mode, IRQ configuration, clock selection, and the output-data bit.
  The retained mask is `0x500b`. The disconnected serial input is high, except when a general-purpose output drives it low.
  Transfer-start bit 7 and multiplayer/UART mode bit 13 return explicit errors.
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

## BIOS reset

RegisterRamReset always clears the low halfword of SIODATA32, even without bit 5.
The upper halfword remains unchanged in this functional subset.
With bit 5 selected, the firmware clears SIOCNT and SIODATA8, selects general-purpose inputs, and clears the supported Joybus reset registers.
It uses ordinary ARM bus stores, not a host-side reset bypass.

Original tests cover independent byte lanes, input pull-ups, output directions, reset flag selection, and explicit unsupported operations.
Whole-access and block-store validation prevent a rejected control value from committing preceding bytes or registers.
RCNT selection bits share the high byte. Each high-byte write replaces the complete selection, so validation needs no retained-mode shadow.
Mode-gating tests cover every high-byte value, previous modes, low-byte preservation, CPU stores, DMA retries, and unchanged interrupt state.
Inactive configuration bits do not wake HALT. RCNT word accesses still reject the unmapped padding at `0x04000136`.

## References

- [GBATEK reset functions](https://problemkaputt.de/gbatek-bios-reset-functions.htm): reset flags, general-purpose selection, and the unconditional SIODATA32 side effect.
- [GBATEK normal serial mode](https://problemkaputt.de/gbatek-sio-normal-mode.htm): data widths, input status, control bits, and writable unused RCNT bits 8/14.
- [GBATEK](https://mgba-emu.github.io/gbatek/): GBA general-purpose pin directions, internal pull-ups, SI falling-edge interrupts, and the SIO mode-selection table.
- [mGBA BIOS implementation at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): functional serial reset defaults, not copied source.

Exact firmware write ordering and hardware pin timing remain unverified.
