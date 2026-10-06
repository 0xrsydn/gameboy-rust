# Local Emerald startup: device reset

## Initial failure

A user-supplied local Emerald ROM stopped in the original BIOS replacement during `RegisterRamReset(0xff)`.
The BIOS deliberately rejected serial/sound flags and executed `INVALID_ARGUMENT_TRAP` (`0xe7f000f1`).
This was an unsupported service argument, not an ARM instruction-decoder failure.
No ROM bytes, extracted code, or assets were added to the repository.

## General correction

The reset correction introduced bounded sound and [disconnected serial](../hardware/serial.md) initialization models.
The [sound model](../hardware/audio.md) now also supports Direct Sound, bounded FIFO DMA, both pulse channels, and noise channel 4.
The BIOS performs reset through original ARM stores against that model.
Bit 5 selects general-purpose input pins and clears supported serial reset registers.
Bit 6 disables sound, resets FIFO queues, clears mixing configuration and accessible wave RAM, and sets the functional bias default.
The low halfword of SIODATA32 is cleared unconditionally, matching the documented BIOS side effect.

This is not a game-specific bypass. Original ARM and Thumb callers exercise selected flags and the all-device request.
Tests verify caller preservation, RAM selection, unselected device state, readback masks, and rejected device activity.
Byte, halfword, word, block-store, and DMA validation retain explicit errors without partial register writes.

## Reproduction and current result

Use a local ROM that you may lawfully use:

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --steps 1000000
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --window --frames 120
```

The earlier reset-only implementation stopped at master sound enable (`0x04000084`, PC `0x082e0518`).
The Direct Sound increment then reached a PSG channel 1 trigger (`0x04000065`, PC `0x082e03bc`).
Channel 1 support reached the channel 2 trigger (`0x0400006d`, PC `0x082e03c0`).
Both pulse channels then reached the noise trigger (`0x0400007d`, PC `0x082e03c4`).
Noise channel 4 support then reached an incorrectly rejected RCNT write: `0x0100` at `0x04000134`, PC `0x082e2aac`.
RCNT bit 15 was clear, so bit 8 did not enable general-purpose input/output (GPIO) interrupts.
GBATEK documents bits 8 and 14 as writable but unused in normal mode.
The serial model now retains these inactive bits. Effective GPIO interrupt enable and Joybus selection still return explicit errors.

Both runs pass that write and now stop in the original BIOS replacement:

```text
Steps: 398019; instructions: 396289; IRQ entries: 2; DMA units: 1728; HALT idle: 0
Nominal cycles: 1334271
PC=0x00000378
read-only memory at 0x00000000
```

The native run captures five startup frames. These do not establish a working title screen or gameplay.
Audio-device behavior is validated by original tests, not by audible game output.
The window run does not reach its requested frame limit and correctly exits with failure.
Terminal execution is also diagnostic, not a compatibility pass.
Results apply to this local file; no cartridge revision or checksum was inferred from its filename.

## Validation and next requirement

Workspace debug/release tests, strict Clippy, public ARM/Thumb/memory/BIOS checkpoints, and pinned Pong scenarios pass.
Native ROM-window tests and graphics smoke modes pass on Darwin arm64.
The pinned Pong debug/release reports match.

The next investigation is the BIOS replacement's attempted write to address zero at PC `0x00000378`.
Trace the service arguments and original firmware routine before deciding whether the caller or service behavior is wrong.
The diagnostic alone does not establish the root cause. Do not make BIOS memory writable to bypass it.
The corrected RCNT write does not establish a need for active link transfers or GPIO interrupts.

Noise now has deterministic counter clocks and shares tested length/envelope logic with the independent pulse channels.
Sweep applies only to channel 1. Direct Sound has FIFO clocks and nominal DMA requests.
All supported channels use the instantaneous digital-level inspection interface.
Wave channel 3, a continuous sample stream, and desktop output are still missing.
Do not treat digital-level tests as audible-game validation or suppress remaining diagnostics without implementing their behavior.
Cartridge saves remain a separate required feature. SRAM alone will not support Emerald's Flash save protocol.

Hardware and emulator references are recorded in the linked subsystem documents.
