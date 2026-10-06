# BIOS boot graphics initialization

## Observed failure

The MIT-licensed ZeroDayArcade Pong homebrew booted and serviced VBlank interrupts without a CPU diagnostic.
Version 2 ROM scenarios then found a black captured screen despite running menu and gameplay logic.
The same checks passed after firmware initialized the affine scales. The ROM bytes and assertions did not change.

The source selects Mode 3 and BG2 but does not write an affine matrix.
Our raw I/O state starts with zero coefficients. The original boot sequence previously left those values unchanged.
Zero PA and PD make the renderer sample the same source pixel across the screen.
This is a firmware initialization gap, not a reason to bypass affine rendering for bitmap modes.

## Correction and independent regressions

The original BIOS now executes ARM stores of 256 to BG2PA, BG2PD, BG3PA, and BG3PD before entering the cartridge.
Fresh device state already supplies zero PB, PC, and origins.
The change does not alter `Memory::new`, external firmware, or SoftReset behavior.
It does not claim complete hardware initialization or exact startup timing.

Original test programs select a video mode without writing matrices.
Separate horizontal and vertical pixels establish the expected identity transform for bitmap graphics and both affine backgrounds.
A captured frame checks the timed rendering path. A raw-memory control retains the zero-matrix behavior.
The bitmap and affine startup tests failed before the correction and pass afterward.

## Compatibility evidence

Pong revision `c784b6036a4f188c50932b411e98126bfcbd07d6` passes bounded menu, settings, return, match-start, ball-motion, and paddle press/release checks.
Debug and release scenario reports match on Darwin arm64.
A bounded native Pong window run and the original native-window smoke test complete successfully.
The automated input checks use the core input interface; they do not establish manual keyboard playthrough or full-game correctness.

The pinned public ARM, Thumb, memory, and BIOS checkpoints still pass in both profiles.
Additional boot instructions intentionally change startup step/cycle counters and internal firmware addresses.
No public ROM, game asset, or third-party implementation source is included in this repository.

## References

- [Pinned Pong source and license](https://github.com/ZeroDayArcade/Pong-Homebrew-GBA/tree/c784b6036a4f188c50932b411e98126bfcbd07d6): `main.c` selects Mode 3; `graphics.h` defines rectangles and text positions.
- [Pinned mGBA I/O initialization](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/io.c): `GBAIOInit` supplies identity PA/PD values.
- [Pinned mGBA RegisterRamReset](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): independent cross-check of affine reset values, not copied implementation.
- [gbadoc register summary](https://gbadev.net/gbadoc/registers.html) and [GBATEK](https://mgba-emu.github.io/gbatek/): affine coefficient roles and register addresses.

These sources support functional initialization. They are not a hardware trace of Nintendo firmware startup.
