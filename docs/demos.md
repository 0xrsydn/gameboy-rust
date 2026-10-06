# Demos and controls

Every demo runs original test content through the `gameboy-rust` desktop executable. None of them load games.
Keyboard mapping to GBA buttons lives in `crates/desktop/src/desktop.rs`.

## Diagnose prefetch cancellation timing

```sh
direnv exec . cargo run --locked -p gba-demos --example prefetch_cancellation > /tmp/prefetch-cancellation.csv
```

This headless original ARM probe compares published read-cancellation observations with two separate emulator measurements.
Instruction-boundary totals match, but actual timer samples currently fail. The example therefore exits with status 1.
Do not treat the matching totals as a hardware-timing pass.
See [the protocol, source hashes, and timer sampling gap](research/prefetch-cancellation.md).
The probe needs no downloaded ROM, assets, window, or external assembler.

## Open the affine raster demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --affine-raster-demo
```

The emulated CPU builds a scanline offset table during VBlank.
HBlank direct memory access (DMA) changes BG2PB after each visible row.
Internal affine origins accumulate these changes, producing horizontal distortion across the tiled background.

- Arrows: pan.
- Q: rotate.
- W: zoom.
- Z: bypass distortion.
- Enter: reset panning.
- Escape: exit.

Click the window to give it keyboard focus. Q, W, and Z can be combined.
This uses original test content. It does not load games.

## Open the scanline raster demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --raster-demo
```

The CPU configures repeated HBlank DMA to change the backdrop palette after each visible row.
The captured frame shows horizontal color bands. A whole-frame snapshot would show only one color.

- Left/Right: move the color bands.
- Enter: reset their position.
- Escape: exit.

Click the window to give it keyboard focus.
All CPU-driven demos now present captured rows rather than reconstructing the screen at VBlank.
This is row-level capture, not pixel-accurate GBA rendering. Emerald is still unsupported.

## Open the mosaic demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --mosaic-demo
```

The CPU cycles background and sprite mosaic blocks from 1×1 to 16×16 pixels.
The size changes every eight VBlank updates, then repeats.

- Arrows: scroll the backgrounds.
- Z: hold to bypass background mosaic.
- X: hold to bypass sprite mosaic. Hold Z and X together to bypass both.
- Q: rotate the sprite. W: zoom the sprite. These controls can be combined.
- Enter: reset scrolling. Escape exits.

Z and X do not flip the sprite or change its priority in this demo.
Click the window to give it keyboard focus.
The CPU writes MOSAIC and the layer-enable flags. The host only supplies input and presents captured frames.

## Open the window and color-effects demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --effects-demo
```

The CPU moves a rectangular display window over the original tile scene.
Colors inside the rectangle remain unchanged. Colors outside become brighter by default.

- Arrow keys: scroll and move the rectangle by two pixels per update.
- Z: hold for alpha blending outside the rectangle.
- X: hold to darken outside the rectangle. Z takes precedence over X.
- Enter: reset scrolling and the rectangle position. Escape exits.
- Q/W retain sprite rotation and zoom. Z also flips the regular sprite; X lowers its background priority.

Click the window to give it keyboard focus.
The CPU writes window bounds and effect registers during VBlank. The host only supplies input and presents captured frames.
This demo keeps its settings stable during visible rows. The core also supports row-level changes, but not pixel-accurate timing.

## Open the page-flipping bitmap demos

From a logged-in macOS desktop session:

```sh
direnv allow .
# Mode 4: 240×160 pixels selected from a palette.
direnv exec . cargo run --locked --release -- --bitmap4-demo

# Mode 5: 160×128 RGB555 pixels, centered without automatic stretching.
direnv exec . cargo run --locked --release -- --bitmap5-demo
```

Each demo uses two original bitmap images. The emulated CPU switches pages every 32 updates during VBlank.
The demos copy both images at startup; they demonstrate page selection, not continuous drawing into the hidden page.

- Arrow keys: pan by two source pixels per frame.
- Q: hold for 45-degree clockwise rotation.
- W: hold for approximately 2× zoom. Q and W can be combined.
- Z: hold to force page 1. Release to resume automatic page selection.
- Enter: reset panning. Escape or the window close button exits.

Click the window to give it keyboard focus. The host supplies input and presents captured frames; it does not switch pages.

## Open the affine background demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --affine-demo
```

This Mode 2 demo rotates and scales an original tiled background around the screen center.
The emulated CPU copies the palette and tiles, then updates affine registers during VBlank.

- Arrow keys: pan by two source pixels per frame. Opposite directions cancel.
- Q: hold for 45-degree clockwise rotation.
- W: hold for approximately 2× zoom. Combine Q and W to rotate and zoom.
- Z: hold to disable map wrapping. Out-of-map pixels show the blue backdrop.
- Enter: reset panning. Escape or the window close button exits.

Click the window to give it keyboard focus. The host supplies input and presents captured frames; it does not transform pixels.

## Open the CPU-driven tile demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --tile-demo
```

This Mode 0 demo shows an original red-and-white sprite above grass, water, and transparent gold crosses.
Two backgrounds scroll at different speeds. The sprite stays at the center.
The emulated CPU copies assets through BIOS services and updates scrolling and sprite attributes during VBlank.
The host supplies buttons and displays captured frames. It does not write video RAM or sprite attributes.

- Arrow keys: scroll by two pixels per frame. The map wraps at its edges.
- Q: hold to rotate the sprite 45 degrees clockwise.
- W: hold to enlarge the sprite approximately 2×. Combine Q and W to rotate and enlarge it.
- Z: hold to flip the sprite horizontally while Q and W are released.
- X: hold to put the sprite behind the gold crosses, but above the terrain.
- Enter: reset scrolling.
- Escape or the window close button: exit.

Click the window to give it keyboard focus. Presentation targets 59.73 Hz.
This is test content, not Pokémon or a game port.

## Open the CPU-driven bitmap demo

From a logged-in macOS desktop session:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --graphics-demo
```

The emulated CPU uses direct memory access (DMA) channel 3 to fill a blue background.
It then draws a 16×16 square using GBA Mode 3.
It reads the GBA `KEYINPUT` register to move or recolor the square.
The native window displays 240×160 pixels at 4× scale.

- Arrow keys: move by two pixels per emulated frame.
- Z: GBA A, changes the square to red.
- X: GBA B, changes the square to green. A takes priority if both are pressed.
- Enter: GBA Start, resets the square to the center.
- Backspace: GBA Select; Q/W: GBA L/R. These reach KEYINPUT but have no demo action.
- Escape or the window close button: exit.

Click the window if input has no effect. Losing focus releases all emulated buttons.
This is original test code, not a game ROM. It uses the optional original BIOS replacement's `VBlankIntrWait` service before redrawing.
Presentation targets 59.73 Hz. Each visible row samples at HBlank; the completed frame becomes available at VBlank.

### Original host display test

Run without arguments to open the earlier display test:

```sh
direnv exec . cargo run --locked --release
```

The window displays a 240×160 framebuffer at 4× scale, with a 960×640 content area.
It shows color bars, a gradient, a moving line, and a controllable square.
The presentation loop targets 60 updates per second. This is not GBA hardware timing.

- Arrow keys: move the square.
- Space: pause or resume the moving line. The status indicator changes from green to red when paused.
- Escape or the window close button: exit.

Click the window if keyboard input has no effect.
These default-window controls only test the desktop interface. Use `--graphics-demo` for emulated input and graphics.

Other commands:

```sh
# Terminal-only CPU demo; does not open a window.
direnv exec . cargo run --locked -- --cpu-demo

# Run a timer-generated interrupt through an original test handler.
# Uses nominal instruction/bus costs and opcode prefetch; sub-instruction device scheduling is not modeled.
direnv exec . cargo run --locked -- --timer-demo

# Verify 60 CPU-driven frames with scripted movement and color changes.
direnv exec . cargo run --locked --release -- --graphics-smoke-test

# Verify 60 page-flipping frames in each new bitmap mode.
direnv exec . cargo run --locked --release -- --bitmap4-smoke-test
direnv exec . cargo run --locked --release -- --bitmap5-smoke-test

# Verify 60 affine background frames, including pan state and every pixel.
direnv exec . cargo run --locked --release -- --affine-smoke-test

# Verify 60 CPU-driven tile frames, including scroll state and every pixel.
direnv exec . cargo run --locked --release -- --tile-smoke-test

# Open the host display test, submit 60 frames, then exit automatically.
direnv exec . cargo run --locked --release -- --smoke-test

# Show command-line help.
direnv exec . cargo run --locked -- --help
```
