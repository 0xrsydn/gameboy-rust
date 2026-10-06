# Game Pak GPIO and RTC control subset

The core supports an explicitly selected cartridge GPIO interface and RTC command/control transactions.
GPIO means general-purpose input/output. RTC means real-time clock.
Calendar access, clock advancement, battery persistence, RTC interrupts, and other cartridge sensors remain unimplemented.
Unsupported operations return diagnostics. No fabricated date or successful calendar transfer is reported.

## Selection

`Memory::set_cartridge_hardware(CartridgeHardware::Rtc)` attaches the bounded interface with initial state.
`CartridgeHardware::None` is the default. Selecting either option replaces peripheral state, without changing ROM bytes.
Call this API between machine steps. The core does not inspect filenames, game headers, or library signatures to select hardware.

The desktop enables this interface with `--rtc`, in either ROM execution mode:

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --steps 2000000
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --window --frames 60
```

The report states that only GPIO and command/control are supported.
The JSON suite schema does not yet select cartridge peripherals. Its default remains no attached peripheral.
There is no host-clock dependency, ROM patch, save file, or automatic device detection.

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
Control parameters transfer least-significant bit first.
During reads, the RTC supplies each bit on the falling edge. GPIO reads do not advance the serial state.

Using GBATEK's MSB-first command representation:

- `0x62` writes one control byte; `0x63` reads one control byte.
- `0x60` and `0x61` reset supported control state to zero. Calendar reset side effects are not implemented.
- `0x64` through `0x67` return an explicit calendar-access diagnostic.
- `0x6c` and `0x6d` return an explicit force-interrupt diagnostic.
- Unused commands and invalid command prefixes remain diagnostic.

Initial control is `0x40`, a deterministic powered 24-hour-mode setting.
Bit 6 is writable. Unused bits and read-only bit 7 do not retain writes.
Writes enabling bits 1, 3, or 5 fail because their interrupt or uncertain control behavior is not implemented.
No power-failure condition is synthesized. There is no calendar value behind this control setting yet.

A partial control write does not commit. A complete byte commits at its final rising edge.
Extra clocks beyond the documented parameter length remain diagnostic rather than guessing repeated transfers.
Selecting with clock low, driving data during read clocks, and changing data at a sampling edge also remain diagnostic.
Minimum physical setup/hold durations, signal races, and exact analog behavior are not modeled.

## Bus integration and tests

CPU and DMA accesses retain Game Pak wait-state costs.
Machine steps stage cartridge state separately from ROM bytes. Failed steps discard pin edges and control changes.
Block-store preflight applies earlier pin/direction changes before validating later accesses.
A later diagnostic leaves earlier RAM, GPIO, and CPU state unchanged.
Data reads and instruction fetches share the selected register overlay. Fetches have no serial side effects.

Original tests cover selection, masks, directions, retained latches, read enable, widths, ROM preservation, and command framing.
They check control reads/writes/reset, aborts, both command-byte representations, edge stability, and unsupported commands.
ARM/Thumb, DMA3, block preflight, staged fetches, retryable errors, and nominal bus costs have separate regressions.
File-backed desktop tests check explicit `--rtc` selection, unchanged files, option rejection, and native-window propagation.
These tests validate the bounded implementation, not physical RTC timing or game playability.

## References

- [GBATEK cartridge GPIO](https://problemkaputt.de/gbatek-gba-cart-i-o-port-gpio.htm): register addresses, masks, directions, read control, and uncertain byte-store behavior.
- [GBATEK GBA RTC](https://problemkaputt.de/gbatek-gba-cart-real-time-clock-rtc.htm): control fields, reset, calendar formats, and command mapping relative to the DS chip.
- [GBATEK DS RTC](https://problemkaputt.de/gbatek-ds-real-time-clock-rtc.htm): the linked serial flow, chip-select sequencing, bit edges, and forward/reverse command representations. GBA register differences remain separate.
- [mGBA GPIO at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/cart/gpio.c): comparison for retained outputs, direction changes, idle inputs, serial edges, and initial control. Its host-clock lookup, ROM-buffer mutation, and repeated transfers are not copied.
- [mGBA memory at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/memory.c): first-window halfword GPIO write routing. This is not physical proof of all address aliases.
- [Explaining GBA RTC](https://bmchtech.github.io/post/rtc/): introductory background surfaced during research; the author explicitly warns that details may be inaccurate. Register and protocol decisions use the references above.
