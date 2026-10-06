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
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --steps 2000000
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --window --frames 60
```

The earlier reset-only implementation stopped at master sound enable (`0x04000084`, PC `0x082e0518`).
The Direct Sound increment then reached a PSG channel 1 trigger (`0x04000065`, PC `0x082e03bc`).
Channel 1 support reached the channel 2 trigger (`0x0400006d`, PC `0x082e03c0`).
Both pulse channels then reached the noise trigger (`0x0400007d`, PC `0x082e03c4`).
Noise channel 4 support then reached an incorrectly rejected RCNT write: `0x0100` at `0x04000134`, PC `0x082e2aac`.
RCNT bit 15 was clear, so bit 8 did not enable general-purpose input/output (GPIO) interrupts.
GBATEK documents bits 8 and 14 as writable but unused in normal mode.
The serial model now retains these inactive bits. Effective GPIO interrupt enable and Joybus selection still return explicit errors.

That correction reached `read-only memory at 0x00000000` inside the original BIOS replacement, PC `0x00000378`.
A bounded local trace found a Thumb `CpuSet` call with source `0x02022a74`, destination zero, and count `0x1c` halfwords.
The caller supplied zero before entering the service. The failing original BIOS instruction was a halfword store.
The copy routine did not corrupt the destination. Its stores encountered an overly strict memory-bus diagnostic.

The [mapped-BIOS write correction](../hardware/bios.md#writes-to-the-mapped-bios) now discards CPU/DMA writes without modifying firmware.
The original copy service still executes all its reads and writes. No ROM patch or service-specific bypass was added.
Host writes remain strict. Reference-emulator comparisons and original tests support this bounded bus behavior.
The trace stayed local; no game code or assets were added to the repository.

The copy correction reached a serial-transfer start at PC `0x082e6e5c`.
A local register trace found RCNT zero and a SIOCNT write of `0x5084`: 32-bit mode, IRQ enabled, external clock selected.
With no connected clock source, this request must wait rather than complete on elapsed CPU cycles.
The [serial subset](../hardware/serial.md#external-clock-waiting) now retains the request without shifting data or raising a serial IRQ.

External-clock waiting then reached internal-clock selection at PC `0x082e6eb8`, with start still set.
The serial model now handles that unshifted transition and clocks normal 8/32-bit transfers at nominal rates.
It shifts disconnected high input, clears start at completion, and requests serial IRQs through the ordinary device transaction.
No connected partner, fabricated peer data, or immediate completion was added.

Normal internal clocks then reached multiplayer selection (`SIOCNT=0x2000`, PC `0x082e4358`).
The disconnected multiplayer model now accepts that configuration and reports idle child status.
A child cannot start itself without a parent clock. No linked transfer, assigned player ID, or completion IRQ is fabricated.
The send register supports 16-bit data; receive latches do not become invented peer data.
See [the register model and evidence limits](../hardware/serial.md#disconnected-multiplayer-configuration).

Multiplayer configuration then reached a Game Pak GPIO control write (`0x080000c8`, PC `0x082e29f8`).
The optional [cartridge GPIO/RTC interface](../hardware/cartridge-gpio.md) first added read enable, directions, pin latches, and RTC control transactions.
That reached a calendar-read command at PC `0x082e28a6`.
The calendar model now supports date/time reads, complete writes, reset, and caller-supplied elapsed seconds.
Core tests inject deterministic time. Desktop `--rtc` uses UTC startup and monotonic elapsed time, including during GBA STOP.
Select it explicitly; ordinary ROM bytes and other ROMs' default hardware remain unchanged.

Both terminal and native runs with `--rtc` now reach a Flash unlock write:

```text
Steps: 964375; instructions: 961679; IRQ entries: 80; DMA units: 2616; HALT idle: 0
Nominal cycles: 3546961
PC=0x082e1892
unmapped memory at 0x0e005555
```

The native run captures twelve startup frames. These do not establish a working title screen or gameplay.
Audio-device behavior is validated by original tests, not by audible game output.
The window run does not reach its requested frame limit and correctly exits with failure.
Terminal execution is also diagnostic, not a compatibility pass.
Results apply to this local file; no cartridge revision or checksum was inferred from its filename.

## Validation and next requirement

Workspace debug/release tests, strict Clippy, public ARM/Thumb/memory/BIOS checkpoints, and pinned Pong scenarios pass.
Native ROM-window tests and graphics smoke modes pass on Darwin arm64.
The pinned Pong debug/release reports match.

The next requirement is a cartridge Flash save device, beginning with command unlock and identification.
The failing state contains value `0xaa` and address `0x0e005555`, matching the documented unlock sequence.
Research device selection, IDs, banking, program/erase, busy behavior, and safe persistence before claiming save compatibility.
RTC persistence and cartridge IRQ behavior remain separate missing features.
Ordinary ROM writes remain read-only. Without `--rtc`, the earlier GPIO diagnostic is still expected.
This result does not establish gameplay, linked transfers, or cartridge/serial GPIO interrupt support.

Noise now has deterministic counter clocks and shares tested length/envelope logic with the independent pulse channels.
Sweep applies only to channel 1. Direct Sound has FIFO clocks and nominal DMA requests.
All supported channels use the instantaneous digital-level inspection interface.
Wave channel 3, a continuous sample stream, and desktop output are still missing.
Do not treat digital-level tests as audible-game validation or suppress remaining diagnostics without implementing their behavior.
Cartridge saves remain a separate required feature. SRAM alone will not support Emerald's Flash save protocol.

Hardware and emulator references are recorded in the linked subsystem documents.
[GBATEK backup Flash](https://problemkaputt.de/gbatek-gba-cart-backup-flash-rom.htm) identifies the `0xaa`/`0x5555` unlock and chip-identification sequence.
The bounded failure establishes an access requirement, not the device's capacity or full save compatibility.
