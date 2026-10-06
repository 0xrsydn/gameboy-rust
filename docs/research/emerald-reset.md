# Local Emerald startup: device reset

## Initial failure

A user-supplied local Emerald ROM stopped in the original BIOS replacement during `RegisterRamReset(0xff)`.
The BIOS deliberately rejected serial/sound flags and executed `INVALID_ARGUMENT_TRAP` (`0xe7f000f1`).
This was an unsupported service argument, not an ARM instruction-decoder failure.
No ROM bytes, extracted code, or assets were added to the repository.

## General correction

The core now provides a bounded [disabled sound](../hardware/audio.md) and [disconnected serial](../hardware/serial.md) initialization model.
The BIOS performs reset through original ARM stores against that model.
Bit 5 selects general-purpose input pins and clears supported serial reset registers.
Bit 6 disables sound, clears mixing configuration and accessible wave RAM, and sets the functional bias default.
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

Both runs now pass the BIOS reset and stop at the game's sound activation write:

```text
PC=0x082e0518
unsupported sound master enable (audio engine not implemented): write 0x8f at 0x04000084
```

The native run captures startup frames before this diagnostic. These are not evidence of a working title screen or gameplay.
The window run does not reach its requested frame limit and correctly exits with failure.
Terminal execution is also diagnostic, not a compatibility pass.
Results apply to this local file; no cartridge revision or checksum was inferred from its filename.

## Validation and next requirement

Workspace debug/release tests, strict Clippy, public ARM/Thumb/memory/BIOS checkpoints, and pinned Pong scenarios pass.
Native ROM-window tests and graphics smoke modes pass on Darwin arm64.
The pinned Pong debug/release reports match.

The next observed startup requirement is active audio-device behavior, starting at SOUNDCNT_X master enable.
Implement channel/FIFO clocks, state, DMA requests, and an output interface before claiming sound support.
Do not make enable succeed solely to suppress this diagnostic.
Cartridge saves remain a separate required feature. SRAM alone will not support Emerald's Flash save protocol.

Hardware and emulator references are recorded in the linked subsystem documents.
