# Game Pak GPIO and RTC calendar subset

The core supports an explicitly selected cartridge GPIO interface and RTC calendar/control transactions.
GPIO means general-purpose input/output. RTC means real-time clock.
An explicit caller supplies initial time and elapsed seconds. The core never samples a host clock.
Battery persistence, RTC interrupts, and other cartridge sensors remain unimplemented.
Unsupported operations and invalid calendar payloads return diagnostics.

## Selection

`Memory::set_cartridge_hardware(CartridgeHardware::Rtc)` attaches the bounded interface with initial state.
`CartridgeHardware::None` is the default. Selecting either option replaces peripheral state, without changing ROM bytes.
Call this API between machine steps. The core does not inspect filenames, game headers, or library signatures to select hardware.

The desktop enables this interface with `--rtc`, in either ROM execution mode:

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --steps 2000000
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --window --frames 60
```

The report states the desktop clock policy: UTC at startup, then host elapsed time, without persistence or RTC interrupts.
UTC means Coordinated Universal Time. The host does not convert to local time.
The JSON suite schema does not yet select cartridge peripherals. Its default remains no attached peripheral.
There is no core host-clock dependency, ROM patch, save file, or automatic device detection.

## GPIO registers

| Address | Register | Retained bits |
| --- | --- | --- |
| `0x080000c4` | Data output latch and pin samples | 0–3 |
| `0x080000c6` | Direction: 0=input, 1=output | 0–3 |
| `0x080000c8` | Read enable: 0=ROM reads, 1=register reads | 0 |

All other register bits read zero when read enable is set. Initial direction, data latch, and read enable are zero.
With read enable clear, reads return the original supplied ROM bytes, including nonzero bytes.
Disabling reads does not disable writes or reset an RTC transaction.
ROM bytes are never overwritten with register values.

Data writes retain an output latch independently of direction.
Output pins read their driven latch bits. Changing direction can expose a retained output and create an RTC signal edge.
The RTC mapping uses bit 0 for clock, bit 1 for bidirectional data, and bit 2 for chip select.
The bounded input model holds clock, select, and the unused pin low; RTC data is high outside a read transaction.
This input policy follows the pinned reference implementation, not independent physical measurements.
No solar sensor, rumble, gyro, or cartridge interrupt source is attached.

Aligned halfword writes are supported. A word at `0x080000c4` applies data, then direction, as two ordered halfwords.
A rejected second halfword leaves both registers unchanged under the core's diagnostic policy.
A word at `0x080000c8` reaches ordinary ROM at `0x080000ca` and fails without changing control.
Byte writes are diagnostic because the consulted bus-width description is uncertain about GBA versus DS behavior.
Byte, halfword, and word reads use ordinary little-endian lanes, including register/ROM boundaries.

Only the documented first-window addresses are implemented.
Other ROM windows still expose supplied ROM bytes; their writes remain read-only diagnostics.
GPIO mirroring and writes at alias addresses have not been independently validated.
Without explicit hardware selection, all these addresses remain ordinary read-only cartridge memory.

## RTC command and control transfers

Chip select rising with clock high begins a command. Chip select falling aborts the current transaction.
Command bits enter on rising clock edges after software establishes data while clock is low.
The first four command bits on the wire must be `0,1,1,0`.
GBATEK's forward and reverse command-byte representations describe this same wire sequence, not interchangeable wire prefixes.
Control and calendar parameters transfer least-significant bit first.
During reads, the RTC supplies each bit on the falling edge. GPIO reads do not advance the serial state.

Using GBATEK's MSB-first command representation:

- `0x62` writes one control byte; `0x63` reads one control byte.
- `0x60` and `0x61` reset control to zero and the calendar to year 00, month/day 01, weekday/time zero.
- `0x64` writes seven date/time bytes; `0x65` reads them.
- `0x66` writes three time bytes; `0x67` reads them.
- `0x6c` and `0x6d` return an explicit force-interrupt diagnostic.
- Unused commands and invalid command prefixes remain diagnostic.

Initial control is `0x40`, a deterministic powered 24-hour-mode setting.
Bit 6 is writable. Unused bits and read-only bit 7 do not retain writes.
Writes enabling bits 1, 3, or 5 fail because their interrupt or uncertain control behavior is not implemented.
No power-failure condition is synthesized. Core-only attachment initializes the calendar to the documented reset date.
Changing hour mode changes its wire representation without changing the stored time.

A partial control write does not commit. A complete byte commits at its final rising edge.
Extra clocks beyond the documented parameter length remain diagnostic rather than guessing repeated transfers.
Selecting with clock low, driving data during read clocks, and changing data at a sampling edge also remain diagnostic.
Minimum physical setup/hold durations, signal races, and exact analog behavior are not modeled.

## Calendar registers and transactions

Date/time payload order is year, month, day, weekday, hour, minute, second.
Time-only payloads contain hour, minute, second. Numeric fields use binary-coded decimal (BCD).
Each BCD byte stores a decimal tens digit in its upper nibble and a units digit in its lower nibble.
The weekday remains an independent counter from zero through six; software can choose its naming convention.

The supported date range is 2000–2099. Month lengths and leap years follow that range's Gregorian calendar.
Hours use `00`–`23` in 24-hour mode and `00`–`11` in 12-hour mode.
GBA hour bit 7 indicates PM. In 24-hour reads it reflects hours 12–23; writes ignore it.
In 12-hour mode, noon is `0x80`, not `0x12`. Midnight is zero.
Documented unused bits are masked. Invalid BCD digits, out-of-range fields, and impossible dates remain diagnostic.
The S-3511A datasheet describes invalid-data corrections, including delayed seconds correction. Those behaviors are not implemented.

A read snapshots the selected registers at command completion.
Later caller time updates or reseeding do not change the current serial payload. The next command observes the updated calendar.
A write buffers its entire payload and commits only at its final rising edge.
Dropping chip select before completion discards that payload. A rejected final edge can be retried without partial calendar changes.
Time-only writes preserve the live date and weekday at commit, including a midnight crossed during the transaction.
This is a bounded transaction model, not physical verification of chip latching, partial writes, or oscillator phase.

## Clock ownership

The core exposes these APIs on `Memory`:

- `rtc_datetime()` inspects the live calendar, or returns `None` when no RTC is attached.
- `set_rtc_datetime(RtcDateTime)` replaces the calendar between machine steps without changing pending read snapshots.
- `advance_rtc_seconds(u64)` applies elapsed battery-clock seconds between machine steps.
  It changes neither CPU cycles nor interrupt state, and it cannot wake HALT or STOP.

`RtcDateTime::new` validates decimal calendar fields. `from_seconds_since_2000` provides checked epoch conversion with Sunday=0.
Calendar advancement is bounded for any `u64` duration, without iterating once per second.
The two-digit year wraps from 99 to 00 while weekday continues independently. This is the bounded century-wrap policy.
Epoch construction rejects values outside 2000–2099 rather than wrapping a supplied host date.

CPU execution, `advance_cycles`, HALT, and STOP do not advance RTC time automatically.
A caller can advance the battery clock while GBA clocks are stopped. Core tests inject deterministic dates and durations.
Reset changes calendar registers but does not reset the caller's elapsed-time source or its fractional phase.
Exact oscillator startup, subsecond reset/write phase, century-wrap behavior, and mode-switch timing lack physical validation.

The desktop adapter samples `SystemTime` once for UTC startup, at whole-second resolution.
It then supplies elapsed whole seconds from `Instant`, preserving the fractional remainder in its elapsed-time measurement.
Guest time writes and reset therefore remain effective; each update does not replace them with a fresh host date.
Later system-clock adjustments do not change the running RTC. Host dates outside the supported range produce an error.

Terminal runs synchronize between bounded groups of machine steps and at exit.
Native windows synchronize between execution slices, including while waiting for input in STOP.
Host sleeping can advance the RTC on the next update but never advances CPU/device cycles.
This is a coarse host-clock adapter, not cycle-exact pin scheduling or a guarantee about system-suspend behavior.
There is no battery save file or offline catch-up. Each new `--rtc` process starts from host UTC.

## Bus integration and tests

CPU and DMA accesses retain Game Pak wait-state costs.
Machine steps stage cartridge state separately from ROM bytes. Failed steps discard pin edges and control changes.
Block-store preflight applies earlier pin/direction changes before validating later accesses.
A later diagnostic leaves earlier RAM, GPIO, and CPU state unchanged.
Data reads and instruction fetches share the selected register overlay. Fetches have no serial side effects.

Original tests cover selection, masks, directions, retained latches, read enable, widths, ROM preservation, and command framing.
They check control/calendar reads, complete writes, reset, aborts, both command-byte representations, and unsupported commands.
Calendar tests cover every day in the supported century, hour modes, PM flags, leap days, midnight, large advances, and invalid payloads.
Read snapshots, live-date time writes, independent HALT/STOP advancement, and failed final-edge rollback have separate checks.
ARM/Thumb, DMA3, block preflight, staged fetches, retryable errors, and nominal bus costs have separate regressions.
File-backed desktop tests execute original ARM calendar writes/reads with `--rtc` in terminal and native-window modes.
The probe places executable code outside the GPIO overlay. Host-adapter tests inject epoch values and elapsed durations without sleeping.
These tests validate the bounded implementation, not physical RTC timing or game playability.

## References

- [Seiko S-3511A datasheet](https://www.rockby.com.au/DSheets/10110.pdf), sections 2-1, 3-3, and 4: BCD calendar fields, weekday counter, reset values, PM behavior, and invalid-data corrections. Chip variants and physical transfer timing remain unverified; unsupported corrections are not silently approximated.
- [GBATEK cartridge GPIO](https://problemkaputt.de/gbatek-gba-cart-i-o-port-gpio.htm): register addresses, masks, directions, read control, and uncertain byte-store behavior.
- [GBATEK GBA RTC](https://problemkaputt.de/gbatek-gba-cart-real-time-clock-rtc.htm): control fields, reset, calendar formats, and command mapping relative to the DS chip.
- [GBATEK DS RTC](https://problemkaputt.de/gbatek-ds-real-time-clock-rtc.htm): the linked serial flow, chip-select sequencing, bit edges, and forward/reverse command representations. GBA register differences remain separate.
- [mGBA GPIO at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/cart/gpio.c): comparison for retained outputs, direction changes, idle inputs, serial edges, and initial control. Its host-clock lookup, ROM-buffer mutation, and repeated transfers are not copied. The calendar implementation uses the documented GBA PM bit, including 24-hour readback; the pinned implementation does not supply that flag.
- [mGBA memory at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/memory.c): first-window halfword GPIO write routing. This is not physical proof of all address aliases.
- [Explaining GBA RTC](https://bmchtech.github.io/post/rtc/): introductory background surfaced during research; the author explicitly warns that details may be inaccurate. Register and protocol decisions use the references above.
