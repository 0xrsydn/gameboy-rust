# Cartridge Flash and save files

The core supports explicit Macronix Flash selection, identification, bank selection, byte programming, and sector/chip erase.
Operations complete after nominal delays and expose bounded data polling. They do not complete immediately on the command write.
The desktop can load and persist raw images through explicit `--save-file PATH` selection.
Original ROM tests verify save/restart/load across processes. Saving in an external game remains unverified.
SRAM, EEPROM, and other manufacturers' protocols remain separate work.

## Selection and core API

Use `--save-type flash64` or `--save-type flash128` in terminal or window ROM modes.
Selection is independent of `--rtc`. Omitting the save type leaves save memory unmapped.
No filename, ROM signature, or first-write detection changes the selected device.
Without `--save-file`, the selected device starts erased (`0xff`) and all changes remain volatile.
The ROM suite schema still has no save-selection or save-file field.

Core APIs on `Memory`:

- `set_save_device(SaveDevice)` selects `None`, `Flash64`, or `Flash128` and initializes an erased array.
  GPIO/RTC state is preserved.
- `load_save_image(&[u8])` accepts exactly the selected capacity. It resets commands, pending operations, ID mode, bank, and modification tracking.
  Invalid size or missing hardware changes nothing.
- `save_image()` inspects the full committed array, independent of bank and ID mode. No selected device returns `None`.
- `save_modified()` reports whether a completed operation changed bytes since setup or loading.
  It remains set after host persistence; hosts compare against their last persisted image to avoid redundant writes.
- `save_write_pending()` reports an incomplete command or operation. Hosts must not persist an exit snapshot in this state.

Configuration, loading, and host inspection occur between machine steps.
The core has no file, clock, window, or host-audio dependency. Frontends supply image bytes and own durable storage.
A browser frontend can use the same API with browser storage instead of filesystem paths.

## Bus and commands

Only `0x0e000000` through `0x0e00ffff` are mapped. Accesses are byte-only.
Halfword/word transfers, DMA, instruction fetches, and unverified aliases remain diagnostic.
CPU Flash reads must execute from external or internal work RAM, following GBATEK's access description.
Reads from ROM/BIOS code remain diagnostic. Host inspection has no execution restriction; CPU writes can execute from ROM.
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

Erase confirmation is `0x10` at `0x0e005555` for the entire chip, or `0x30` at a 4 KiB sector base.
Sector erase affects only the selected bank's sector. Chip erase affects both banks on Flash128.
Byte programming can clear bits or retain their value. Requests that set a stored zero bit are diagnostic; erase first.
Invalid commands, prefixes, bank values, and sector addresses retain the preceding state for diagnostic retry.
This policy does not reproduce hardware behavior for every malformed sequence.

| Device | Capacity | Manufacturer byte | Device byte |
| --- | --- | --- | --- |
| Flash64 | 64 KiB | `0xc2` | `0x1c` |
| Flash128 | 128 KiB | `0xc2` | `0x09` |

Identification exposes these bytes at offsets zero and one. Other ID addresses remain diagnostic.
Bank selection survives ID entry/exit; incomplete unlock reads retain the previous mode and bank.

For Flash64, direct `0xf0` at `0x0e005555` resets commands/ID state or cancels a busy operation.
Cancellation preserves the old array. Partial physical programming on cancellation is not modeled.
After `0xa0`, `0xf0` at that address is program data, not reset.
For Flash128, direct reset is accepted only as a redundant no-op while idle in array mode.
Flash128 requires an unlocked ID exit; direct busy/partial-command cancellation remains diagnostic.

## Nominal completion and transactions

Byte programming takes 650 nominal cycles. Sector and chip erase each take 30,000 nominal cycles.
These are simulation constants, not measured Macronix timings. Chip erase duration is deliberately not capacity-dependent.
GBATEK leaves average Macronix operation times unknown. The constants match the pinned mGBA timing approximation below.
No matching manufacturer datasheet or physical timing measurement establishes these values.

While busy, reads are accepted only at the operation's polling address:

- Program: the target byte.
- Sector erase: the selected sector base.
- Chip erase: `0x0e000000`.

Busy reads return the complement of the final data's bit 7 (DQ7), with other bits zero.
Toggle/error bits, protection, wear, queued sector erase, and suspend/resume are not modeled.
Other busy reads and writes remain diagnostic, except the documented Flash64 cancellation subset.
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
Existing raw images must contain exactly 65,536 or 131,072 bytes, matching the selected hardware.
Loading occurs before the first machine step. A missing file starts an erased array without creating a file immediately.
Directories, symlinks, read-only files, and paths that alias the ROM are rejected.
The ROM is never opened for writing.

Complete the game's own save operation before closing the window normally or pressing Escape.
The frontend persists changed bytes only on a clean exit:

- Interactive window closure or Escape.
- Reaching a requested window frame limit.
- Reaching a terminal step limit or entering terminal STOP.

An incomplete Flash command/operation refuses persistence and returns an error, even if no bytes changed yet.
CPU, DMA, rendering, window, or reported audio errors do not persist the session image.
There is no autosave, crash recovery, save-state support, or RTC persistence.
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
Original ROM process tests program, restart, reload through an IWRAM byte reader, and verify no persistence after diagnostics.
A native-window test persists on frame-limit exit and reloads in another process.
No game code, user save, firmware, or copyrighted assets are included in the tests.

Next, validate an external game's completed save and load using a disposable save path.
Add independent hardware coverage for timing, status bits, busy addresses, and reset behavior before broadening the model.
Implement SRAM and EEPROM separately when a demonstrated compatibility requirement needs them.

## References

- [GBATEK backup Flash](https://problemkaputt.de/gbatek-gba-cart-backup-flash-rom.htm): capacities, IDs, command sequences, banking, program/erase polling, Macronix reset, byte-only access, and RAM execution.
- [GBATEK](https://mgba-emu.github.io/gbatek/), WAITCNT section: save waits and one-cycle-plus-wait timing.
- [mGBA savedata at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/savedata.c): comparison for controller phases, bank addressing, ID overlays, idle reset, and nominal 650/30,000-cycle delays. Its DQ7 approximation is not hardware evidence. Our array mutation is deferred to transactional completion rather than applied immediately.
- [ares Flash at 9408cb43](https://github.com/ares-emulator/ares/blob/9408cb43d4948fc3ea6e152a307a34348df3fe04/ares/gba/cartridge/flash.cpp): comparison for IDs, banking, unlocked ID exit, and idle reset. Immediate program/erase and permissive unlock handling are not adopted.
