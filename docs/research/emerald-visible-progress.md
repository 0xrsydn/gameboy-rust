# Local Emerald visible-progress probe

## Method

A temporary host driver uses public core APIs (deleted after use; lives
outside version control). It boots the local user-supplied ROM with
`bios::boot`, selects `CartridgeHardware::Rtc` and `SaveDevice::Flash128`,
fixes the RTC to `[24, 1, 1, 1, 12, 0, 0]` without advancing it, enables
scanline rendering, and calls `Machine::run_until_vblank(400000)` once per
frame. Input snapshots apply after each VBlank capture, before the next
machine step. Frames dump as checksums plus a final PPM for visual reading.
No ROM bytes, screenshots, or extracted code are committed.

Three schedules were used:

- No input (attract mode).
- `advance`: periodic A/Start taps (A for 10 of every 120 frames, Start for
  10 of every 120 frames offset by 60) to walk through title, menu, and
  dialogue.
- `sweep`: deterministic rotation holding each button 20 frames in turn.

## Observed visible progress

No-input attract mode renders the title screen, the Pokémon logo, the
NEW GAME / OPTION menu, the Prof. Birch scene, and legible dialogue text
(`Hi! Sorry to keep yo`, `Welcome to the world of POKéMON!`). Menu and
dialogue text, the Birch sprite, and the spotlight composite correctly.

The `advance` schedule reaches further: Birch dialogue, the `And are you?`
gender prompt, and — at 12,000 frames — the in-game start menu (`BAG A SAVE
OPTION EXIT`) over the overworld with the player sprite in a room. Frame
checksums evolve continuously through the run; PC samples alternate between
the `0x080008c6–0x080008cc` wait loop and game code (`0x082e6dcc`,
`0x082e6dce`).

The `sweep` schedule changes captured output per input (input-dependent
execution) but its denser presses stall the intro on a blue field; raw
button rotation is worse than sparse taps for menu progress.
A 40,000-frame `advance` run completed with exit status zero: no diagnostic
through frame 39,999, so the reported `INVALID_ARGUMENT_TRAP` at 37,101
captured frames did not reappear on this input path. Blind taps take a
different menu path than manual play, so the identical starter-selection
scene is not confirmed visited — this validates forward progress and no
regression, not the exact user scenario.
From about frame 9,000 onward the captures cycle among three checksums
(`0x24bdf6c568`, `0x2503d09807`, `0x7983c73704`) with periodic black fade
frames, consistent with Start taps toggling the overworld start menu; the
scene past frame 12,000 was not visually confirmed. The final frame is
black mid-fade (checksum zero).

Execution is diagnostic-free through 40,000 tapped frames (about 3.9
billion machine steps). This establishes visible menus, sprite/dialogue
composition, and input-driven progress — not correct gameplay, audio, or
saving.

## Save-navigation attempt (negative result)

A `save-run` schedule replayed the deterministic `advance` path to the
12,000-frame start-menu state, then pressed Down, Down, A (SAVE), A (YES),
and A again with settling gaps, running to 13,200 frames with Flash image
dumps at exit. The run completed without a diagnostic, but the dumped
131,072-byte image shows no save: bank 0 sectors 0–13 erased, bank 1
sectors 0–11 programmed with structured non-`0xff` data that does not match
an Emerald save layout, and the final captured frame is the overworld with
no save dialog. The pressed menu item was not SAVE (blind taps cannot aim),
or the dialog timing missed. `save_modified` was not observed true.
External-game save/restart/load therefore remains unverified and waits on
manual play with `--save-file`, not on more blind schedules.

## Second-game data point

A local FireRed ROM (16 MiB) faults during boot init, after about 1.19
million steps with `--rtc --save-type flash128`:

```text
PC=0x081e37d2 CPSR=0x0000003f System/Thumb
r0=0x09fe2ffe r1=0x00000020
read-only memory at 0x09fe2ffe
```

A dynamic trace shows boot code spilling `0x09fe20f8`, `0x04000204`
(WAITCNT), and `0x09fe2ffe` to the stack, writing `0x1800` to WAITCNT, then
`STRH r1,[r0]` of `0x20` to `0x09fe2ffe`. That address is ROM window
`0x09` past the 16 MiB EOF; the bus reports `ReadOnly`. On hardware the
`0x09` window mirrors `0x08` and ROM writes are ignored. The following
instruction stream also loads halfwords from `0x09fe20f8`, so reads past
EOF would fault next under the current policy. Without `--save-type` the
same ROM faults earlier at `0x0e005555` (`unmapped memory`), i.e. it also
expects Flash save hardware. Both failures are recorded, not fixed: mirror
folding plus write-ignore semantics need independent hardware evidence and
a deliberate policy decision against the explicit-error convention. See
[the usability roadmap](../usability-roadmap.md) for sequencing.

## References

- [GBATEK memory map](https://problemkaputt.de/gbatek-gba-memory-map.htm):
  three 32 MiB ROM windows at `0x08`/`0x0a`/`0x0c` with wait states 0–2.
- [mGBA memory at c30aaa8f](https://github.com/mgba-emu/mgba/blob/c30aaa8f/src/gba/memory.c):
  out-of-bounds ROM loads log a game error and return `(address >> 1) &
  0xFFFF`; ROM stores go through patch infrastructure. Precedent for
  non-fatal handling, not a hardware measurement. Not copied.
