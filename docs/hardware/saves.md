# Cartridge Flash identification and read subset

The core supports explicit Macronix Flash selection, chip identification, array reads, and 128 KiB bank selection.
Programming, erase, busy polling, and host save-file persistence remain unimplemented.
This is not working game-save support. Unsupported writes fail instead of reporting a completed save.
SRAM, EEPROM, and other manufacturers' protocols remain separate work.

## Selection and initial data

The desktop accepts either `--save-type flash64` or `--save-type flash128` in ROM terminal/window modes.
The option is independent of `--rtc`. Omitting it leaves save memory unmapped.
Unknown values and duplicate options fail before opening the ROM.

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --save-type flash128 --steps 3000000
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --save-type flash128 --window --frames 60
```

Selection is explicit, not inferred from a filename, ROM signature, or the first write.
A 64 KiB device cannot silently become a 128 KiB device.
The desktop starts each selected device with erased `0xff` bytes and reports the read-only subset's limits.
It does not look for, read, create, or overwrite a save file. Existing game saves remain untouched.
The ROM suite schema still uses no save device by default and has no save-selection field yet.

Core APIs on `Memory`:

- `set_save_device(SaveDevice)` replaces save hardware and initializes its array to erased bytes.
  `None`, `Flash64`, and `Flash128` are available. The call preserves GPIO/RTC state.
- `load_save_image(&[u8])` supplies an exact-capacity image and resets command state, ID mode, and bank selection.
  A wrong length or missing device changes nothing. This is host setup, not an emulated program operation.
- `save_image()` inspects the complete array, independently of ID mode or selected bank.
  It returns `None` when no save device is selected.

Configuration and image replacement occur between machine steps. No filesystem operations belong to the core.
Selecting GPIO/RTC hardware does not replace an attached save device or its bytes.
This feature can inspect caller-supplied images but cannot modify their data through Flash commands.

## Bus behavior

Only `0x0e000000` through `0x0e00ffff` are mapped when a device is selected.
The Flash interface uses byte accesses. Halfword/word transfers, DMA, and instruction fetches remain diagnostic.
Unverified aliases outside this window remain unmapped rather than returning mirrored data.
Ordinary ROM writes remain read-only.

CPU Flash reads must execute from external or internal work RAM, as required by GBATEK's access description.
A read from ROM/BIOS code reports a diagnostic rather than inventing a value for an unsupported bus sequence.
Host byte inspection has no CPU execution restriction. CPU command writes may execute from ROM.

WAITCNT bits 0–1 select save wait states 4, 3, 2, or 8.
The nominal byte-access costs are therefore 5, 4, 3, or 9 cycles, for both sequential and non-sequential requests.
Save waits do not alter ROM wait fields. Flash data accesses cancel Game Pak opcode prefetch through the existing nominal model.
Changing only save wait bits preserves the queue until a cartridge access cancels it.
These costs are source-backed estimates, not physical timing measurements.

## Commands

Use these byte addresses and values for the unlock prefix:

1. Write `0xaa` to `0x0e005555`.
2. Write `0x55` to `0x0e002aaa`.
3. Write a command byte to `0x0e005555`.

Supported commands after the prefix:

| Command | Behavior |
| --- | --- |
| `0x90` | Enter chip-identification mode |
| `0xf0` | Exit identification mode and return to array reads |
| `0xb0` | On Flash128 only, accept a bank byte next at `0x0e000000` |

In ID mode, byte zero is the manufacturer and byte one is the device:

| Selection | Manufacturer | Device | Combined ID |
| --- | --- | --- | --- |
| Flash64 | `0xc2` | `0x1c` | `0x1cc2` |
| Flash128 | `0xc2` | `0x09` | `0x09c2` |

Other ID-mode addresses remain diagnostic because their values are not established by this subset.
Array mode reads bytes from the selected 64 KiB bank. Flash64 always uses bank zero.
Flash128 accepts bank values zero and one only. Bank changes do not alter stored data.
Entering and leaving ID mode preserves the selected bank.
Reads during an incomplete unlock retain the previous read mode and bank.

For the documented 64 KiB Macronix device, a direct `0xf0` write at `0x0e005555` resets command/ID state.
For Flash128, the same write is accepted only as a redundant reset when already idle in array mode.
This bounded no-op matches the inspected emulators' idle behavior. It does not establish direct ID exit or busy-command cancellation.
Flash128 still requires the full unlock prefix to exit ID mode. Direct reset during an incomplete unlock remains diagnostic.
No matching manufacturer datasheet or physical measurement was used to claim broader 128 KiB reset behavior.

`0xa0` byte-program setup and `0x80` erase setup remain diagnostic at the command write.
No operation starts, data changes, busy state, completion flag, or interrupt is fabricated.
Unknown commands, invalid prefixes, and invalid bank writes are also diagnostic.
Rejected accesses retain the previous command state and bytes, so retrying the same access gives the same result.
This atomic diagnostic policy does not reproduce hardware handling of every malformed sequence.

## Transactions and validation

The small Flash controller state joins the existing cartridge machine-step transaction.
The full save image stays outside the copied controller state; this read-only subset never mutates it during emulation.
Failed CPU or DMA steps discard speculative controller changes and timing.
Block-store preflight rejects a wide Flash destination before committing earlier RAM or device writes.
HALT/STOP and elapsed cycles cannot create Flash completion events. There is no pending program or erase operation to finish.

Original tests cover both device IDs, every byte in both banks, exact image lengths, erased state, invalid sequences, and unchanged ROM/save data.
They distinguish redundant idle reset from unsupported Flash128 ID/partial-command reset.
ARM/Thumb probes verify ROM command writes, RAM byte reads, wait settings, restrictions, and retryable failed steps.
Additional tests cover staged ID reads, block rollback, DMA rejection, GPIO/RTC independence, and prefetch cancellation.
Desktop tests execute an original IWRAM byte reader with explicit device selection in terminal and native-window modes.
No game code, save file, or copyrighted assets are included in these tests.

## Remaining work

Implement and validate program/erase commands, busy polling, cancellation, and byte-program bit transitions before claiming save functionality.
Then add explicit host save loading and safe persistence, with exact-size validation and restart tests.
Do not overwrite existing files or add immediate-success responses merely to pass a game check.

## References

- [GBATEK backup Flash](https://problemkaputt.de/gbatek-gba-cart-backup-flash-rom.htm): capacities, IDs, unlock sequences, bank selection, 64 KiB Macronix reset, byte-only access, and RAM-execution requirements.
- [GBATEK](https://mgba-emu.github.io/gbatek/), WAITCNT section: save wait fields and one-cycle-plus-wait timing.
- [mGBA savedata at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/savedata.c): comparison for controller phases, bank addressing, ID overlays, and idle reset handling. Its default chip IDs differ; its write/erase timing and busy approximation are not copied.
- [ares Flash at 9408cb43](https://github.com/ares-emulator/ares/blob/9408cb43d4948fc3ea6e152a307a34348df3fe04/ares/gba/cartridge/flash.cpp): comparison for IDs, banking, unlocked ID exit, and unchanged idle state after an unrecognized direct reset. Its immediate program/erase behavior and permissive unlock handling are not adopted.
