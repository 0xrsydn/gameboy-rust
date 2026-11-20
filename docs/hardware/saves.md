# Cartridge Flash, SRAM, EEPROM, and save files

The core supports explicit Macronix Flash selection, identification, bank selection, byte programming, and sector/chip erase.
Operations complete after nominal delays and expose bounded data polling. They do not complete immediately on the command write.
The desktop can load and persist raw images through explicit `--save-file PATH` selection.
Original ROM tests verify save/restart/load across processes. Saving in an external game remains unverified.
The core also supports 32 KiB SRAM and 512-byte or 8 KiB serial EEPROM; see [SRAM](#sram) and [EEPROM](#eeprom).
Other manufacturers' Flash protocols remain separate work.

## Selection and core API

Without `--save-type`, the desktop selects the device from the save library's ID string in the ROM
(`SaveDevice::detect`). Nintendo's save libraries embed a word-aligned ASCII ID followed by a three-digit version
(GBATEK, "GBA Cart Backup IDs"), and emulators commonly key on it:

| ID | Device |
| --- | --- |
| `EEPROM_Vnnn` | `Eeprom`: 512 B or 8 KiB, as the game's first request selects (below) |
| `SRAM_Vnnn`, `SRAM_F_Vnnn` | `Sram` (32 KiB SRAM or FRAM) |
| `FLASH_Vnnn`, `FLASH512_Vnnn` | `Flash64` |
| `FLASH1M_Vnnn` | `Flash128` |

The first aligned ID in ROM order wins; a ROM without one gets no save device. Games built without the library, or
whose ID was stripped, need an explicit type. mGBA 0.10.5 does not scan for these IDs: it picks the type on the first
save access and takes Flash size from its built-in game list, which this repository does not copy.
`--save-type flash64`, `flash128`, `sram`, `eeprom`, `eeprom512`, or `eeprom8k` overrides detection, and
`--save-type none` leaves save memory unmapped. The terminal output names the type and whether it was detected.
Selection is independent of `--rtc`. File extensions and first-write behavior never change the device.
Without `--save-file`, the selected device starts erased (`0xff`) and all changes remain volatile.
The ROM suite schema still has no save-selection or save-file field.

Core APIs on `Memory`:

- `SaveDevice::detect(&[u8])` returns the device a ROM's save-library ID names, or `None`.
- `set_save_device(SaveDevice)` selects `None`, `Flash64`, `Flash128`, `Sram`, `Eeprom`, `Eeprom512`, or `Eeprom8k` and initializes an erased array.
  GPIO/RTC state is preserved.
- `load_save_image(&[u8])` accepts one of `SaveDevice::image_sizes()`: the capacity, or 512 or 8192 bytes for `Eeprom`.
  It resets commands, pending operations, ID mode, bank, and modification tracking.
  Invalid size or missing hardware changes nothing.
- `save_image()` inspects the full committed array, independent of bank and ID mode. No selected device returns `None`.
  For `Eeprom` it is 512 bytes once the game chose the small part, and 8 KiB otherwise.
- `save_modified()` reports whether a completed operation changed bytes since setup or loading.
  It remains set after host persistence; hosts compare against their last persisted image to avoid redundant writes.
- `save_write_pending()` reports an incomplete command or operation. Hosts must not persist an exit snapshot in this state.

Configuration, loading, and host inspection occur between machine steps.
The core has no file, clock, window, or host-audio dependency. Frontends supply image bytes and own durable storage.
A browser frontend can use the same API with browser storage instead of filesystem paths.

## SRAM

`SaveDevice::Sram` is a 32 KiB byte-wide array with no command protocol. It starts erased (`0xff`) like Flash.
The 32 KiB array repeats every 32 KiB through `0x0e000000..0x0fffffff`. The mGBA suite's hardware expectations for
its "SRAM mirror" tests match SRAM exactly at `0x0f000000`, although GBATEK lists `0x0e010000` and up as unused.
Flash keeps its 64 KiB window.
Stores take effect in program order when the step completes; reads later in the same step already see them.
One step can stage sixteen byte stores, enough for an `STM` of every register.
Byte stores and loads may execute from ROM; the Flash work-RAM read restriction does not apply.
SRAM sits on an 8-bit bus that receives the instruction's own, possibly unaligned, address.
CPU halfword and word loads (`LDR`, `LDRH`, `LDRSH`) read that one byte and repeat it on every lane, then apply normal ARM7TDMI rotation.
For example, `LDRH` at offset 1 of `"Ga"` yields `0x61000061`.
CPU halfword and word stores (`STR`, `STRH`) write one byte: the data-bus lane that matches the address.
A word store writes `value >> (8 * (address & 3))`; a halfword store drives both bus halves, so it writes its low byte at even addresses and its high byte at odd ones.
Each access costs one 8-bit bus access.
Aligned results match hardware expectations published with the [mGBA test suite](https://github.com/mgba-emu/suite); unaligned stores were measured only as leaving byte 0 untouched.
`LDM`/`STM` and word swaps use the same lanes for each word. A misaligned `LDM`/`STM` base keeps its low bits on
SRAM's bus, so each word uses base + 4n with those bits; other regions align it down. DMA uses the same lanes too: a DMA1–3 source reads the repeated byte, and a
DMA3 destination stores the address's lane, as the suite's "SRAM load/store" DMA results show. DMA0–2 destinations mask
away from SRAM. Host halfword/word accesses remain diagnostics.
The replacement BIOS's CpuSet/CpuFastSet pass misaligned pointers straight to these instructions, so misaligned calls
give the suite's hardware results.
SRAM shares the save window's WAITCNT wait states, save-image API, host persistence, and save states with Flash.
A save state records the selected device. Restoring it into a machine with another save device is rejected as a cartridge setup mismatch.

## EEPROM

`Eeprom512`, `Eeprom8k`, and `Eeprom` follow GBATEK's serial protocol on Game Pak data bit 0, one bit per halfword.
With ROM up to 16 MiB the part answers anywhere in `0x0d000000..0x0dffffff`; above 16 MiB only `0x0dffff00..0x0dffffff`.
Elsewhere in that window, reads and writes behave as ordinary ROM.

- Read request: `11`, the block address (6 bits, or 14 bits with the low 10 decoded), then `0`.
  The next 68 halfword reads return 4 zero bits, then the 64-bit block MSB first.
- Write request: `10`, the block address, 64 data bits MSB first, then `0`.
  The part then reports busy (bit 0 clear) for a nominal 108,368 cycles before the block is stored.
- Idle and ready status reads return 1. Bits 1–15 read as zero; hardware drives only bit 0.

Block bytes are stored in transfer order: the first data bit is byte 0, bit 7, matching common `.sav` layouts.

A game cannot ask the part for its size (GBATEK: no detection mechanism exists); it is built for one and sends requests
with that address width. `Eeprom` therefore leaves the width open until the first request and takes it from that DMA's
length: 9 (read) or 73 (write) halfwords select 6 address bits and a 512-byte image, 17 or 81 select 14 bits and 8 KiB.
Any other first-request length, or a first request without a DMA, is a diagnostic and decides nothing.
This follows mGBA 0.10.5, which derives a request's address bits from the remaining DMA count
(`GBASavedataWriteEEPROM` in `src/gba/savedata.c`); it is not hardware-verified. Unlike mGBA, which accepts any length
and grows a 512-byte image when an address beyond it appears, the width here is fixed after the first request.
Loading a 512- or 8192-byte image fixes it up front, so an existing save keeps its size.
The decided width is save-state data; the state's setup identity records `Eeprom`, so states restore into any
automatically sized session but not into an explicit `eeprom512` or `eeprom8k` one.
Bitstreams must come from DMA (or host setup writes). GBATEK states manual `STRH` transfers do not work, so CPU stores are diagnostics.
A CPU `LDRH` polls status; a CPU read during a data read-out, byte or word accesses, a missing leading `1`,
a missing trailing `0`, a request while busy, and a request during a read-out are all diagnostics.
Bits shift out only inside machine steps, so a failed step rolls back; host reads between steps only inspect.
An unfinished request, read-out, or write sets `save_write_pending()`, which blocks host persistence.

## Bus and commands

Only `0x0e000000` through `0x0e00ffff` are mapped. Accesses are byte-only.
Halfword/word transfers, DMA, instruction fetches, and unverified aliases remain diagnostic.
GBATEK says Flash reads must execute from work RAM. mGBA 0.10.5 has no such restriction, and neither does this core:
CPU code in any region can read and write Flash.
Ordinary ROM bytes remain read-only.

WAITCNT bits 0–1 select save wait states 4, 3, 2, or 8.
Nominal byte-access costs are therefore 5, 4, 3, or 9 cycles for sequential and non-sequential accesses.
Flash data accesses cancel Game Pak opcode prefetch. Changing save wait bits alone preserves the queue.
These costs are source-backed estimates, not physical measurements.

The unlock prefix consists of these byte writes:

1. Write `0xaa` to `0x0e005555`.
2. Write `0x55` to `0x0e002aaa`.
3. Write the command to `0x0e005555`.

| Command | Behavior after the unlock prefix |
| --- | --- |
| `0x90` | Enter chip identification |
| `0xf0` | Exit identification and return to array reads |
| `0xb0` | Flash128 only: next write selects bank 0 or 1 at `0x0e000000` |
| `0xa0` | Next byte write supplies the target address and program data |
| `0x80` | Accept another `0xaa`/`0x55` unlock pair, then an erase confirmation |

Erase confirmation is `0x10` at `0x0e005555` for the entire chip, or `0x30` at any other address for the 4 KiB
sector that holds it. Sector erase affects only the selected bank's sector. Chip erase affects both banks on Flash128.

Malformed sequences follow mGBA 0.10.5 (`GBASavedataWriteFlash` in `src/gba/savedata.c`); none of this is
hardware-verified:

- Byte programming stores the byte as written. Hardware can only clear bits, so the two agree whenever a game erases first.
- A write that breaks an unlock prefix is ignored and the prefix starts over. An armed erase stays armed.
- Unknown commands, bad bank addresses or values, and any command but `0xf0` in ID mode are ignored.
- A bad erase confirmation at `0x0e005555` drops the erase; a bad value elsewhere keeps it armed.
- Bank 1 on Flash64 stays diagnostic: mGBA grows the chip to 128 KiB, but the selected device fixes the image size here.

| Device | Capacity | Manufacturer byte | Device byte |
| --- | --- | --- | --- |
| Flash64 | 64 KiB | `0xc2` | `0x1c` |
| Flash128 | 128 KiB | `0xc2` | `0x09` |

Identification exposes these bytes at offsets zero and one. Other addresses read the array, as in mGBA.
Bank selection survives ID entry/exit; incomplete unlock reads retain the previous mode and bank.

For Flash64, direct `0xf0` at `0x0e005555` resets commands/ID state or cancels a busy operation.
Cancellation preserves the old array. Partial physical programming on cancellation is not modeled.
After `0xa0`, `0xf0` at that address is program data, not reset.
For Flash128, direct reset is a no-op. It does not exit ID mode or cancel a busy operation, and it breaks a partial
unlock prefix like any other malformed write. Flash128 needs the unlocked ID exit.

## Nominal completion and transactions

Byte programming takes 650 nominal cycles. Sector and chip erase each take 30,000 nominal cycles.
These are simulation constants, not measured Macronix timings. Chip erase duration is deliberately not capacity-dependent.
GBATEK leaves average Macronix operation times unknown. The constants match the pinned mGBA timing approximation below.
No matching manufacturer datasheet or physical timing measurement establishes these values.

While busy, each operation has a polling address:

- Program: the target byte.
- Sector erase: the selected sector base.
- Chip erase: `0x0e000000`.

Busy reads return the complement of the final data's bit 7 (DQ7), with other bits zero.
Toggle/error bits, protection, wear, queued sector erase, and suspend/resume are not modeled.
Other busy reads follow mGBA: bytes in a sector still being erased read `0x5f`, and other bytes read as if the
operation had finished. Busy writes run the command sequence. Starting another program or erase before the previous
one commits remains diagnostic: the core defers one array change at a time, while mGBA applies each immediately.
After completion, ordinary array reads resume. No completion interrupt is generated.

A program starts at the successful byte-store bus phase, after preceding fetch/data clocks.
The small controller and remaining delay participate in the machine-step transaction.
Completed reads in a step see a staged overlay. The full array changes only when the step commits successfully.
Failed CPU/DMA steps and speculative block-store validation discard staged progress, data changes, and modification flags.
There is no full-image copy per instruction.

`Machine` execution and `Memory::advance_cycles` advance completion. CPU-only stepping does not clock devices automatically.
HALT advances pending operations. STOP freezes them under the current nominal device-clock policy.
This does not reproduce a physical Flash chip's independent oscillator during STOP.

## Desktop persistence

Persistence is explicit. A typical local command is:

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --save-type flash128 --save-file roms/pokemon-emerald.sav --window --audio
```

Omit `--audio` for muted execution. This command is not a game-save compatibility claim.
Use a copy of any valuable existing save while compatibility remains unverified.

The save path's parent directory must exist. File extensions do not select the format.
Existing raw images must have the selected device's size: 65,536 or 131,072 bytes for Flash, 32,768 for SRAM,
512 or 8,192 for EEPROM. An automatically sized EEPROM accepts either and keeps it; a new file takes the size the
game's first request chose. A size that does not match is refused, never resized.
Loading occurs before the first machine step. A missing file starts an erased array without creating a file immediately.
Directories, symlinks, read-only files, and paths that alias the ROM are rejected.
The ROM is never opened for writing.

Complete the game's own save operation before closing the window normally or pressing Escape.
The frontend persists changed bytes only on a clean exit:

- Interactive window closure or Escape.
- Reaching a requested window frame limit.
- Reaching a terminal step limit or entering terminal STOP.

An incomplete Flash command/operation refuses persistence and returns an error, even if no bytes changed yet.
A ROM window that ends on a CPU, DMA, rendering, window, or reported audio error also persists changed bytes, so completed in-game saves outlive the diagnostic.
The diagnostic remains the session's error. A refused or failed write there is reported as a line of output instead of replacing it.
A failed save-state restore leaves save memory undefined and never writes the file.
Terminal runs keep the stricter rule: a diagnostic does not persist the session image.
There is no autosave, crash recovery, or RTC persistence. [Save states](save-states.md) are a separate, whole-machine feature and do not replace the game's own save.
The emulator cannot detect a game's multi-command save transaction. Exiting between completed commands can still save incomplete game data.
A crash or forced termination loses changes since startup and can leave a stale lock.

The host storage sequence is:

1. Reserve a sibling `.lock` file exclusively for the entire session.
2. Before persistence, compare the current destination bytes with the loaded baseline. Refuse ordinary external changes.
3. Write and sync a uniquely named same-directory `.tmp.<pid>.<counter>` file.
4. Before replacement, write and sync the prior bytes to a unique `.bak.<pid>.<counter>` backup.
5. Recheck the destination, then install the image. Existing files use atomic rename; new files use a no-clobber hard link.
6. Remove the temporary name and sync the parent directory. Release the owned lock on normal scope exit.

Unchanged images create no save or backup. Backups are never overwritten or automatically deleted by this frontend.
Backup files are ordinary files, not OS-enforced immutable files. Review and manage them manually when needed.
A directory-sync failure after installation reports that the save was installed but durability is uncertain.
Storage exhaustion or permission errors are explicit failures; previous backups are retained.

The lock coordinates cooperating emulator processes. It is not protection against hostile or noncooperating filesystem changes.
The final comparison and rename are not an atomic compare-and-swap operation.
After a crash, confirm that no emulator owns the save, inspect the save/backup files, then remove the stale `.lock` manually.
The frontend never takes over an existing lock automatically.
Persistence currently requires a Unix host. Darwin arm64 is validated; Linux and browser storage remain unvalidated.

## Validation and remaining work

Original tests cover all program byte values in both banks, erase scope, exact delays, polling, cancellation, and bit transitions.
ARM/Thumb tests cover store timing, completion reads, failed-step rollback, block preflight, HALT, and STOP.
File tests cover size checks, unchanged images, locks, external changes, aliases, read-only paths, and retained backups.
Original ROM process tests program, restart, reload through an IWRAM byte reader, and verify no persistence after terminal diagnostics.
File tests also cover window-diagnostic persistence, its reported refusals, and the failed-restore exclusion.
A native-window test persists on frame-limit exit and reloads in another process.
No game code, user save, firmware, or copyrighted assets are included in the tests.

Next, validate an external game's completed save and load using a disposable save path.
Add independent hardware coverage for timing, status bits, busy addresses, and reset behavior before broadening the model.
Validate detection and automatic EEPROM sizing against a game that uses EEPROM.

## References

- [GBATEK backup IDs](https://problemkaputt.de/gbatek-gba-cart-backup-ids.htm): the save-library ID strings and their word alignment.
- [GBATEK backup Flash](https://problemkaputt.de/gbatek-gba-cart-backup-flash-rom.htm): capacities, IDs, command sequences, banking, program/erase polling, Macronix reset, byte-only access, and RAM execution.
- [GBATEK](https://mgba-emu.github.io/gbatek/), WAITCNT section: save waits and one-cycle-plus-wait timing.
- [mGBA savedata at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/savedata.c): comparison for controller phases, bank addressing, ID overlays, idle reset, and nominal 650/30,000-cycle delays. Its DQ7 approximation is not hardware evidence. Our array mutation is deferred to transactional completion rather than applied immediately.
- [mGBA savedata at 0.10.5](https://github.com/mgba-emu/mgba/blob/0.10.5/src/gba/savedata.c) `GBASavedataReadFlash`/`GBASavedataWriteFlash` and [`src/gba/memory.c`](https://github.com/mgba-emu/mgba/blob/0.10.5/src/gba/memory.c) `GBALoad8`: the malformed-sequence, ID, busy-read (`0x5f` while a sector settles), and any-region read behavior followed above.
- [ares Flash at 9408cb43](https://github.com/ares-emulator/ares/blob/9408cb43d4948fc3ea6e152a307a34348df3fe04/ares/gba/cartridge/flash.cpp): comparison for IDs, banking, unlocked ID exit, and idle reset. Immediate program/erase and permissive unlock handling are not adopted.
