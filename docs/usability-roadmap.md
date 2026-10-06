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

## Development loop

For each feature, inspect the source and hardware evidence, then write original regression tests.
Implement the smallest general behavior that satisfies the requirement.
Run focused tests, workspace validation, and relevant native-window checks through `direnv exec .`.
Update behavior notes and compatibility evidence. Save a semantic jj change with its own bookmark.
Then select the next demonstrated requirement without waiting for another confirmation.

Stop for missing access, a necessary product decision, or a destructive operation that needs approval.
Do not replace unsupported hardware with game-specific patches or silent register stubs.
Keep external ROMs, firmware, implementation source, and assets outside version control.
