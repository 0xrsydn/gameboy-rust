# General GBA usability roadmap

The goal is a general-purpose GBA emulator, not a port of one game.
Pokémon Emerald is one possible future compatibility target. It does not define hardware behavior.
Nintendo DS support remains a separate phase.

## Usability criteria

A usable release must demonstrate these results with several lawfully obtained games:

- Boot into a menu and enter gameplay without an unsupported-hardware diagnostic.
- Present stable graphics in the native Darwin window.
- Respond to button presses, held buttons, and releases.
- Preserve supported cartridge saves across process restarts without damaging existing files.
- Produce audio through an isolated host output layer.
- Exit cleanly and report unsupported behavior clearly.

A bounded run without a diagnostic is not proof of playability.
A test pass establishes only the configured assertions, not complete game compatibility.

## Work order

1. Establish repeatable gameplay checks using licensed public homebrew.
   Add VBlank completion, button schedules, and captured-pixel assertions to the ROM suite runner.
2. Exercise menus and gameplay in the native window. Record the exact ROM revision and checksums.
3. Select another independent program. Fix demonstrated boot, graphics, or input failures with original regressions.
4. Implement cartridge save devices and safe host persistence. Verify saving and reloading across processes.
5. Implement audio devices and host output. Keep host dependencies outside `gba-core`.
6. Expand the compatibility matrix across games and hardware features.
   Refine timing when independent tests or gameplay failures show a requirement.

SRAM, Flash, and EEPROM require different protocols. Supporting one does not establish support for the others.
Audio needs device clocks, mixing, and buffering; accepting sound-register writes alone is not audio support.
The detailed hardware limits remain in [status.md](status.md).

## Current gameplay baseline

The [pinned Pong homebrew](public-pong.md) passes scripted menus, ball movement, and paddle press/release checks.
The native Darwin window also starts and presents frames without a diagnostic.
This exposed and corrected missing firmware affine-scale initialization, without a game-specific patch.
Frame-based scenarios and the preparation adapter now make the result repeatable.
A local Emerald run now passes its initial all-device reset, master sound enable, and pulse/noise triggers.
It also passes normal serial transfers and idle multiplayer configuration.
With `--rtc --save-type flash128`, it passes Game Pak GPIO setup, RTC reads, and Flash identification.
It also passes disconnected Joybus configuration and completes a bounded 600-frame native run without a diagnostic error.
That run does not verify screen contents, button responses, or gameplay.
A [fixed-time input probe](research/emerald-wave.md) then exposed an unsupported wave trigger with its gate disabled.
Single-bank wave support now passes that point and completes 1,320 frames with scheduled Start/A input.
Captured output differs from a no-input control; readable menus and gameplay remain unverified.
The core has tested Direct Sound, both pulse channels, single-bank wave channel 3, and noise channel 4.
Fixed-rate stereo capture and opt-in Darwin `--audio` output now work with original native probes.
A local Emerald run submits nonzero device frames but records underruns; listening quality and correct music remain unverified.
Two-bank wave playback, hardware PWM sampling, and improved reconstruction/pacing remain missing.
See [the runtime result](research/emerald-reset.md). Next, verify visible menu progress rather than treating frame counts as playability.
Nominal Flash programming/erase and explicit clean-exit persistence now pass original save/restart/load tests across processes.
A native-window save also reloads through a separate process on Darwin arm64. External-game saving remains unverified.
SRAM is byte-addressable battery-backed save memory. Flash and EEPROM remain separate protocols.

[Paperdomo101/2048-GBA at ae36800d](https://github.com/Paperdomo101/2048-GBA/tree/ae36800dfa1314f96c4c8785b5c2d077fb731aff) is the next independent candidate.
Its CC0 source calls `load_state()` before audio initialization and reads/writes `sram_mem` one byte at a time.
It also uses Maxmod audio, so SRAM alone will not establish compatibility.
This candidate has not been run: the pinned tree has no ROM, GitHub has no release asset, and the linked download returned HTTP 522.
A reproducible source build or an accessible upstream ROM is needed before recording a runtime result.

Next implementation steps:

1. Visible startup/menu progress is established: attract mode reaches the start menu and Birch dialogue, and sparse taps reach the in-game start menu by 12,000 frames ([probe](research/emerald-visible-progress.md)).
   The first demonstrated second-game failure is a boot-init halfword store to ROM mirror `0x09fe2ffe` (recorded, not fixed).
   Use original regressions for any hardware correction; do not invent connected serial devices.
2. Validate an external game's completed save/restart/load cycle using a disposable `--save-file` path.
   Refine nominal Flash timing and polling only with independent evidence and original regressions.
3. Add SRAM separately when the independent candidate requires it; verify widths, mirroring, and waits first.
4. Resume the second-game run and implement demonstrated audio requirements rather than accepting writes without behavior.

## Development loop

For each feature, inspect the source and hardware evidence, then write original regression tests.
Implement the smallest general behavior that satisfies the requirement.
Run focused tests, workspace validation, and relevant native-window checks through `direnv exec .`.
Update behavior notes and compatibility evidence. Save a semantic jj change with its own bookmark.
Then select the next demonstrated requirement without waiting for another confirmation.

Stop for missing access, a necessary product decision, or a destructive operation that needs approval.
Do not replace unsupported hardware with game-specific patches or silent register stubs.
Keep external ROMs, firmware, implementation source, and assets outside version control.
