# Local Emerald startup: device reset

## Initial failure

A user-supplied local Emerald ROM stopped in the original BIOS replacement during `RegisterRamReset(0xff)`.
The BIOS deliberately rejected serial/sound flags and executed `INVALID_ARGUMENT_TRAP` (`0xe7f000f1`).
This was an unsupported service argument, not an ARM instruction-decoder failure.
No ROM bytes, extracted code, or assets were added to the repository.

## General correction

The reset correction introduced bounded sound and [disconnected serial](../hardware/serial.md) initialization models.
The [sound model](../hardware/audio.md) now also supports Direct Sound, bounded FIFO DMA, and both nominal pulse channels.
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
With both pulse channels implemented, both runs now stop at noise channel 4:

```text
Steps: 321163; instructions: 320651; IRQ entries: 0; DMA units: 512; HALT idle: 0
Nominal cycles: 1038746
PC=0x082e03c4
unsupported PSG channel trigger (synthesis not implemented): write 0x80 at 0x0400007d
```

The native run captures three startup frames. These do not establish a working title screen or gameplay.
Audio-device behavior is validated by original tests, not by audible game output.
The window run does not reach its requested frame limit and correctly exits with failure.
Terminal execution is also diagnostic, not a compatibility pass.
Results apply to this local file; no cartridge revision or checksum was inferred from its filename.

## Validation and next requirement

Workspace debug/release tests, strict Clippy, public ARM/Thumb/memory/BIOS checkpoints, and pinned Pong scenarios pass.
Native ROM-window tests and graphics smoke modes pass on Darwin arm64.
The pinned Pong debug/release reports match.

The next observed startup requirement is noise channel 4. Wave channel 3 is also still missing.
Both pulse channels now have independent nominal waveform, length, envelope, status, and stereo mixing behavior.
Sweep applies only to channel 1. The two pulses share the sequencer without sharing their channel state.
Direct Sound has FIFO clocks and nominal DMA requests. All supported channels use the instantaneous digital-level inspection interface.
A continuous sample stream and desktop output are still missing. Do not treat digital-level tests as audible-game validation.
Do not suppress the PSG trigger diagnostic without implementing the requested device behavior.
Cartridge saves remain a separate required feature. SRAM alone will not support Emerald's Flash save protocol.

Hardware and emulator references are recorded in the linked subsystem documents.
