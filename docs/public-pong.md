# Pinned public Pong gameplay baseline

ZeroDayArcade's Pong homebrew now passes scripted menu and gameplay checks on Darwin arm64.
Debug and release reports match. A bounded native window run also completes without a diagnostic.
This is a narrow gameplay baseline, not a complete playthrough or general commercial-game compatibility claim.

## Prepare and run

```sh
direnv exec . python3 -B tools/prepare_homebrew_pong.py /tmp/gba-public-pong-pinned
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-public-pong-pinned/suite.json > /tmp/gba-pong-report.json
direnv exec . cargo run --locked --release -- --rom /tmp/gba-public-pong-pinned/Pong-Homebrew-GBA.gba --window
```

The preparation directory must not already exist. Choose another path for another preparation.
Use `--source-dir PATH` for an offline copy containing every file listed in the lock.
Every source, license, build file, and ROM must match its pinned size and SHA-256 hash before output is created.
Output must stay outside the repository or inside ignored `roms/`. Existing files are never overwritten.
A disk error can leave a partial output directory. Preparation alone does not run or pass the suite.

Window controls:

- Enter or Z selects a menu item (GBA Start or A).
- Up/Down changes the menu selection and moves the player paddle during a rally.
- Escape closes the window.

The game deliberately pauses before the first rally and after scores. Paddle movement starts after the initial pause.
Click the window if it does not receive keyboard input.
The game has no sound or cartridge saves. It does not validate either missing subsystem.

## Provenance

- Repository: [ZeroDayArcade/Pong-Homebrew-GBA](https://github.com/ZeroDayArcade/Pong-Homebrew-GBA).
- Revision: `c784b6036a4f188c50932b411e98126bfcbd07d6`.
- License: MIT; the adapter retains the upstream license in the local output directory.
- Lock: [`tools/homebrew-pong.lock.json`](../tools/homebrew-pong.lock.json).
- ROM: upstream `Pong-Homebrew-GBA.gba`, not a locally rebuilt binary.

The repository contains only original adapter logic, sparse assertions, metadata, and documentation.
No upstream font arrays, implementation source, ROM bytes, or other assets are included.
Build-source correspondence is not established through a reproducible rebuild; the committed upstream ROM is hash-pinned separately.

## Assertion design

All cases start from a fresh machine with the original BIOS replacement.
The manifest uses [version 2 gameplay checks](rom-tests.md#manifest-version-2-gameplay-checks), with a separate machine-step budget for every case.
Button snapshots apply after the numbered VBlank capture and remain held until replaced.
Coordinates and colors come from the pinned `main.c`, `graphics.h`, and font definitions, not captured-image golden files.

| Case | Completion VBlank | Evidence |
| --- | --- | --- |
| Menu | 30 | Title pixel and Play cursor visible; Settings cursor absent |
| Settings | 50 | Down/A changes the heading to the rules screen and moves the cursor |
| Back | 70 | Start returns to the main heading with Settings still selected |
| Start | 40 | Start displays both blue paddles, the green ball, and the white center line |
| Ball motion | 160 | Initial ball point is cleared and a later point is green |
| Up | 175 | Holding Up moves the paddle above its initial position |
| Release | 185 | Releasing Up retains that position instead of continuing movement |
| Down | 205 | Holding Down returns the paddle to its initial vertical range |

Game-start cases press Start at VBlank 30 and release at 32.
The source initializes a 120-iteration pause before ball movement.
The motion checks sample interior pixels to tolerate a small draw-boundary difference; they do not establish exact CPU timing.
Up is held from VBlank 160 to 170. Down is held from 190 to 200.
At two pixels per update, these intervals move the paddle by 20 pixels in opposite directions.
The release case checks both occupied and cleared points to detect continued movement.

## Failure found and corrected

Before the [BIOS boot graphics correction](research/boot-video.md), all gameplay cases failed their visible-pixel assertions.
CPU-only runs had reached the VBlank loop without an error, which concealed the black-screen failure.
The ROM selected Mode 3 without setting affine coefficients. Our firmware had left its scale at zero.
Original ARM boot stores now initialize BG2/BG3 identity scales. No ROM patch or game-specific renderer behavior was added.
The same pinned ROM and assertions pass after the correction.

## Limits and next coverage

Sparse pixels do not validate every glyph, complete frames, collision behavior, scoring, or the victory/reset loop.
The native-window run checks startup and presentation, not a manual keyboard playthrough.
Automated input uses the core button interface. Original native-window tests separately check keyboard mapping and session behavior.
The baseline exercises Mode 3, libgba startup, Thumb/ARM execution, VBlank IRQs, BIOS waits, and button edges.
Select another independent program to expose different graphics, audio, and cartridge requirements.
