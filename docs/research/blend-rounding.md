# Brightness rounding: an open hardware question

The brightness-decrease effect (BLDCNT mode 3) darkens a color channel `x` (0–31) by `x·EVY/16`. When that is not a
whole number, emulators round it differently, and no published hardware result decides between them:

| Emulator | Darken | Brighten |
| --- | --- | --- |
| mGBA 0.10.5 (this emulator's default, `DarkenRounding::Up`) | `x − ((x·EVY) >> 4)`: rounds the result up | `x + (((31 − x)·EVY) >> 4)` |
| Mesen 2, ares (`DarkenRounding::Down`) | `(x·(16 − EVY)) >> 4`: rounds the result down | same as mGBA |
| NanoBoyAdvance | `x − ((x·EVY + 7) >> 4)`: rounds to nearest | `x + (((31 − x)·EVY + 8) >> 4)` |

NanoBoyAdvance also blends green as six bits, taking the color's unused bit 15 as the lowest; for the gray colors below
(bit 15 clear) that can move green, but not red or blue, by half a level. Brightness increase gives the same result in
mGBA and Mesen for every input; darkening differs in 416 of the 544 (color, EVY) pairs, always by one level. Alpha
blending (`(a·EVA + b·EVB) >> 4`) is the same in this emulator, mGBA, and Mesen.

The difference is visible in games: in the [GBA Eval](../gba-eval.md) corpus, Waimanu's and Chip Advance's fades are one
level brighter here than in Mesen, the benchmark's reference.

## Searched, 2026-11-23

Nothing decides it. [GBATEK](https://problemkaputt.de/gbatek.htm) gives `I = I − I·EVY` and says that "the formula
used by nintendo has some additional rounding errors" without stating them; [gbadoc](https://gbadev.net/gbadoc/registers.html)
and [Tonc](https://www.coranac.com/tonc/text/gfx.htm) stop at the formula. None of the public hardware test suites
(destoer, jsmolka, NanoBoyAdvance's `hw-test`, the mGBA suite) checks it; the mGBA suite's results are identical under
both roundings. ares (`ares/gba/ppu/dac.cpp`) blends with black like Mesen; NanoBoyAdvance's rounding came in its
2023 commits "PPU: round blending result to nearest" and "PPU: respect the 6-th green channel bit during blending",
neither of which cites a capture.

## The test ROM

The CPU cannot read a composed pixel back, so the answer has to be seen. `gba_demos::blend_rounding` builds an original
ROM that shows it on one screen:

```sh
direnv exec . cargo run --locked -p gba-demos --example write_blend_rounding_rom -- /tmp/blend-rounding.gba
```

The screen has four bands of seven groups. Each group is three adjacent blocks: on the left and right the two whole
numbers around the exact result, drawn without any effect; in the middle a gray `x` with the band's effect applied. The
middle block equals exactly one neighbor, so on a correct emulator or on hardware the edge to that neighbor disappears
and the other edge stays. The comparison happens on one screen, so the capture's own color handling (a camera, a Game
Boy Player's filter, a capture card's gamma) cannot change which edge vanishes. The CPU switches BLDCNT and BLDY at
lines 40, 80, 120, and 160, under a blank tile row at the top of each band.

Its header leaves the Nintendo logo blank. Real hardware refuses to boot it that way: run devkitPro's `gbafix` on a copy
before copying it to a flash cartridge. Emulators boot it unchanged.

Validated 2026-11-23: this emulator, mGBA 0.10.5 (`mgba-ref`), and Mesen 2 (`mesen-ref`) each show exactly the side
their formula predicts; `gba_demos` tests check this emulator under both settings.

## Reading a capture

For each group, note which edge of the middle block disappears. Digital capture (a GBA with a video-output mod, or a
Game Boy Player with GCVideo) settles every group, and green separately from red and blue. A photo of a backlit screen
settles the lighter groups; the darkest ones (band 2) are hard to see. Both edges visible means hardware rounds in a
way none of these emulators does.

| Band | Effect | EVY | Group | x | Left | Right | mGBA | Mesen, ares | NanoBoyAdvance |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Darken | 3 | 1 | 4 | 3 | 4 | right | left | left |
| 1 | Darken | 3 | 2 | 8 | 6 | 7 | right | left | right |
| 1 | Darken | 3 | 3 | 12 | 9 | 10 | right | left | right |
| 1 | Darken | 3 | 4 | 15 | 12 | 13 | right | left | left |
| 1 | Darken | 3 | 5 | 20 | 16 | 17 | right | left | left |
| 1 | Darken | 3 | 6 | 24 | 19 | 20 | right | left | right |
| 1 | Darken | 3 | 7 | 28 | 22 | 23 | right | left | right |
| 2 | Darken | 11 | 1 | 4 | 1 | 2 | right | left | left |
| 2 | Darken | 11 | 2 | 8 | 2 | 3 | right | left | right |
| 2 | Darken | 11 | 3 | 12 | 3 | 4 | right | left | right |
| 2 | Darken | 11 | 4 | 15 | 4 | 5 | right | left | right |
| 2 | Darken | 11 | 5 | 20 | 6 | 7 | right | left | left |
| 2 | Darken | 11 | 6 | 24 | 7 | 8 | right | left | right |
| 2 | Darken | 11 | 7 | 28 | 8 | 9 | right | left | right |
| 3 | Brighten | 5 | 1 | 4 | 12 | 13 | left | left | left |
| 3 | Brighten | 5 | 2 | 8 | 15 | 16 | left | left | left |
| 3 | Brighten | 5 | 3 | 12 | 17 | 18 | left | left | right |
| 3 | Brighten | 5 | 4 | 16 | 20 | 21 | left | left | right |
| 3 | Brighten | 5 | 5 | 20 | 23 | 24 | left | left | left |
| 3 | Brighten | 5 | 6 | 24 | 26 | 27 | left | left | left |
| 3 | Brighten | 5 | 7 | 28 | 28 | 29 | left | left | right |
| 4 | Brighten | 13 | 1 | 4 | 25 | 26 | left | left | right |
| 4 | Brighten | 13 | 2 | 8 | 26 | 27 | left | left | right |
| 4 | Brighten | 13 | 3 | 12 | 27 | 28 | left | left | left |
| 4 | Brighten | 13 | 4 | 16 | 28 | 29 | left | left | left |
| 4 | Brighten | 13 | 5 | 20 | 28 | 29 | left | left | right |
| 4 | Brighten | 13 | 6 | 24 | 29 | 30 | left | left | right |
| 4 | Brighten | 13 | 7 | 28 | 30 | 31 | left | left | left |

Bands 1 and 2 separate all three emulators; bands 3 and 4 separate NanoBoyAdvance from the other two.

## Until then

`Memory::set_darken_rounding` selects the rounding. The core keeps mGBA's (`Up`), following its mGBA-parity default;
the [GBA Eval build](../gba-eval.md) uses Mesen's (`Down`), because that benchmark scores against Mesen. Neither is a
hardware claim. When a capture exists, the hardware's rounding becomes the only behavior and the setting goes away.
