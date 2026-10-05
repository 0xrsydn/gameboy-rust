# Video: backgrounds, sprites, windows, and effects

Rendering behavior implemented in `crates/gba-core/src/video/`, plus button input registers.

## Graphics and button input

`DISPCNT` at `0x04000000` selects the display mode. The CGB mode bit is forced to zero.
`GREENSWAP` at `0x04000002` exchanges the green channels of adjacent pixel pairs when bit zero is set.
`KEYINPUT` at `0x04000130` exposes ten active-low buttons. Unused bits read as zero; writes are ignored.
`Memory::set_buttons(Buttons)` supplies pressed-high host input. The I/O layer converts it to active-low register bits.
`KEYCNT` and keypad IRQs are not implemented; accesses to KEYCNT return unmapped-memory diagnostics.

Video RAM starts at `0x06000000` and contains 96 KiB.
Its 128 KiB mirror layout repeats the final 32 KiB in offsets `0x18000..0x1ffff`.
Palette RAM starts at `0x05000000` and repeats every 1 KiB.
Halfword and word writes preserve little-endian order.
Byte writes to palette or background video RAM duplicate the byte into the addressed halfword.
Byte writes to object video RAM are ignored. Its start is `0x14000` in bitmap modes and `0x10000` in tile modes.
Object attribute memory (OAM) starts at `0x07000000` and repeats every 1 KiB across the `0x07` region.
OAM supports byte reads and halfword/word reads and writes. Byte writes do nothing.
Its 128 eight-byte entries hold three sprite attributes and one affine-parameter halfword each.
All bytes start at zero. Software must disable unused sprites; zeroed entries describe active 8×8 sprites.
CPU and DMA accesses share this memory. Active-display access restrictions are not modeled.

`Memory::render_frame` creates a snapshot without advancing the emulated clock.
Mode 3 stores 38,400 RGB555 pixels at `0x06000000..0x06012bff` in row-major order.
The renderer samples that bitmap through BG2's matrix and reference-point registers.
Pixel bit 15 and the DISPCNT page-select bit do not affect Mode 3 output. Black pixels are opaque.
Disabling BG2 leaves palette entry zero behind sprites. Forced blank displays white, regardless of the selected mode.
All bitmap modes support regular and affine sprites, windows, and color effects.
Enabling backgrounds other than BG2 in bitmap modes produces errors without changing the output buffer.
Reserved display modes 6 and 7 return errors. Modes 0 through 5 support snapshots and row capture, not complete hardware implementations.

Mode 4 stores 240×160 byte-sized palette indices. Index zero is transparent and can reveal a lower-priority sprite.
A nonzero palette index remains opaque even when its palette color is black.
Palette changes affect the next captured row or debug snapshot, not rows already captured.
Mode 5 stores 160×128 RGB555 pixels. All in-bounds pixels are opaque, including black. Pixel bit 15 is ignored.
Mode 5 does not automatically stretch to the LCD size. BG2's affine matrix can scale or position the bitmap.

Modes 4 and 5 select their visible page with DISPCNT bit 4:

| Mode | Page 0 pixel data | Page 1 pixel data |
| --- | --- | --- |
| 4 | `0x06000000..0x060095ff` | `0x0600a000..0x060135ff` |
| 5 | `0x06000000..0x06009fff` | `0x0600a000..0x06013fff` |

Both page bases are 40 KiB apart. Mode 4's padding after each image is not pixel data.
Writes to the hidden page do not change visible pixels until that page is selected.
Mode 4 byte writes still duplicate the byte into both halves of a halfword, as required by the video memory bus.
Palette data and sprite tiles are shared between pages. Bitmap-mode sprite tiles start at `0x06014000`.

Mode 0 supports all four text backgrounds, with these features:

- `BG0CNT..BG3CNT` at `0x04000008..0x0400000f`: priority, character base, palette depth, map base, and size.
- Horizontal/vertical scroll latches at `0x04000010..0x0400001f`, with byte-write merging and nine-bit offsets.
- Scroll registers are write-only. Reads return zero as a placeholder; open bus is not modeled.
- Control masks are `0xdfcf` for BG0/BG1 and `0xffcf` for BG2/BG3.
- 8×8 tiles with 4-bit palette indices and sixteen banks, or 8-bit indices with one palette.
- Horizontal and vertical tile flips, and ten-bit tile numbers.
- 256×256, 512×256, 256×512, and 512×512 maps assembled from 32×32-tile screen blocks.
- Scroll coordinates wrap at map edges. The affine overflow bit does not change text backgrounds.
- Index zero is transparent in every palette bank. Nonzero indices can display opaque black.
- Lower priority numbers win. Equal priorities select BG0 before BG1, BG2, and BG3.
- Uncovered pixels use palette entry zero. Green swap runs after composition.

Mode 0 supports mosaic independently on each background. Rendering errors leave the output buffer unchanged.
Background tile or map fetches outside the first 64 KiB return diagnostics.
This is not a complete picture processing unit (PPU).

Affine background snapshots support these configurations:

- Mode 1: text BG0/BG1 and affine BG2.
- Mode 2: affine BG2/BG3.
- Modes 3, 4, and 5: affine bitmap BG2, with out-of-bitmap samples always transparent.
- Enabling a background unavailable in the selected mode returns a diagnostic rather than silently ignoring it.

`BG2PA..BG2PD` occupy `0x04000020..0x04000027`. BG2 reference points X/Y occupy `0x04000028..0x0400002f`.
BG3 uses the corresponding registers at `0x04000030..0x0400003f`.
Coefficients are signed 16-bit values divided by 256. Reference points are signed 28-bit values divided by 256.
All registers are write-only; reads return zero as an open-bus placeholder.
Byte and halfword writes merge into the stored values. Reference-point bits 28–31 are ignored.
Registers reset to zero, not an identity matrix. Software must initialize PA and PD to 256 for identity sampling.
The existing Mode 3 demo now performs that initialization through ARM stores.

The snapshot source coordinates are `X + PA*x + PB*y` and `Y + PC*x + PD*y`, with fixed-point scaling.
The renderer floors fractional coordinates after summing, including negative coordinates.
Mode 0 text backgrounds ignore these affine registers. Affine backgrounds ignore text scroll registers.

Affine tile backgrounds use flat maps of byte-sized tile indices, without text-map flips or palette-bank attributes.
Every tile is 8×8 with eight-bit palette indices, regardless of BGCNT bit 7.
Map sizes are 128×128, 256×256, 512×512, or 1024×1024 pixels.
BGCNT bit 13 selects wrapping or transparent edges in Modes 1 and 2. All bitmap modes ignore this bit.
Palette index zero is transparent in tiled backgrounds and Mode 4. Mode 3/5 black bitmap pixels remain opaque.
Background priorities, sprite composition, and green swap apply after sampling.
Map fetches beyond the supported 64 KiB background area still return diagnostics.

**Timing limit:** debug snapshots use one set of programmed registers for the whole image.
Row capture instead samples from separate internal X/Y origins.
At each visible line end, enabled affine backgrounds add the current PB/PD to their origins.
HBlank DMA coefficient changes therefore affect the next row's increment, not all preceding rows.
BG2 tracks in Modes 1–5; BG3 tracks in Mode 2. Disabled or unavailable backgrounds do not accumulate.
Window masking does not pause tracking. This row-level model also continues tracking during forced blank.

X/Y writes before HBlank replace the current row's origin for the written axis.
Writes after row capture set the next row's origin and override that axis's next increment.
Partial writes merge into programmed values, not accumulated origins. Matrix writes do not reload origins.
Both axes wrap within signed 28-bit fixed-point storage.
The next visible frame reloads programmed origins at the line-zero boundary, including values written during VBlank.
Debug snapshots continue to use programmed origins and absolute screen Y.

Vertical background mosaic holds the origin until the shared background counter becomes zero at a line end.
It then adds the current PB/PD multiplied by the current block height.
Changing the size does not reset the counter or recompute the origin from absolute screen Y.
Tracking continues with capture disabled. Fixed-register clock batches remain constant-time.

These are explicit row-level rules, not verified hardware latch timing.
Exact VBlank reload timing, enable delays, forced-blank restart delays, individual sprite fetches, and background pixel-fetch timing remain unmodeled.

Sprites are enabled with DISPCNT bit 12 in any supported display mode:

- All twelve square, horizontal, and vertical size combinations are supported, from 8×8 to 64×64.
- X coordinates wrap at 512; Y coordinates wrap at 256. Rendering clips to the 240×160 screen.
- Regular-sprite horizontal and vertical flips reverse the whole sprite, including multi-tile sprites.
- Four-bit sprites use sixteen OBJ palette banks. Eight-bit sprites use the complete OBJ palette.
- OBJ palette data starts at `0x05000200`, separate from background palettes.
- Index zero is transparent. A nonzero index can display opaque black.
- DISPCNT bit 6 selects consecutive 1D tile rows or fixed-width 2D tile rows.
- Tile numbers count 32-byte slots from `0x06010000`. Eight-bit tiles occupy two slots.
- In 2D mapping, eight-bit base tile numbers ignore bit zero. In 1D mapping, odd bases remain valid.
- 2D columns wrap within 32 slots. Tile addressing wraps within the 32 KiB OBJ area.
- Modes 3–5 suppress sprites whose starting tile number is below 512, regardless of the displayed page.
- Lower priority numbers win object conflicts. Equal priorities retain the earlier OAM entry's color.
- Transparent texels can change priority and mosaic metadata without replacing the stored color or semi-transparency flag.
- The selected sprite beats a background at equal priority. A background with a lower priority number covers it.
- Disabled regular sprites are skipped, regardless of their other attributes.

Affine sprites use a two-by-two matrix for rotation, scaling, reflection, or shear:

- Attribute 0 bit 8 enables affine sampling. Bit 9 then doubles the drawing area's width and height instead of disabling the sprite.
- Attribute 1 bits 9–13 select one of 32 matrices. These bits no longer control horizontal or vertical flips.
- Matrix coefficients PA, PB, PC, and PD occupy OAM offsets `6`, `14`, `22`, and `30`, plus `32 × matrix index`.
- Each coefficient is a signed 16-bit value divided by 256, also called signed 8.8 fixed point.
- `[256, 0, 0, 256]` gives an identity transform. `[128, 0, 0, 128]` enlarges the image 2×.
- The renderer maps drawing-area coordinates back into the source texture, around their respective centers.
- Fractional coordinates round down after both matrix products are added. Out-of-texture samples are transparent.
- Double-size mode enlarges only the drawing area, not the source texture or the matrix scale.
- X/Y still locate the drawing area's upper-left corner. Software must adjust X/Y to preserve the center when changing area size.
- Sprites can share matrices, including matrices beside disabled sprite entries. Updates affect later preparation events or immediate debug snapshots.
- Zero matrices sample the center texel throughout the drawing area. Singular matrices are valid; no matrix inversion occurs.

Semi-transparent sprites and OBJ windows support both regular and affine sampling.
Prohibited modes/shapes return explicit diagnostics.
Unsupported active sprites are checked even when offscreen. Global OBJ disable skips these checks.
Sprite preparation applies a nominal per-row work allowance. Individual fetch timing and OAM access contention remain unimplemented.
Sprite priority includes transparent-texel metadata updates, but the core has not run public hardware test ROMs.

### Buffered sprite row preparation

The core prepares the following visible sprite row at cycle40 of lines0–158 and line227.
Line227 prepares row0. Lines159–226 do not prepare another visible row.
This is a row-at-once approximation of the hardware's one-line-ahead engine, not an accurate sequence of OAM/VRAM fetches.

Preparation samples OAM attributes, affine matrices, sprite VRAM, tile mapping, bitmap restrictions, and the ahead-of-display OBJ mosaic phase.
It also samples DISPCNT bit 5 to select the row's nominal sprite work allowance.
It stores palette indices, priority, mosaic and semi-transparency metadata, and OBJ-window coverage.
The following row's HBlank composition resolves those indices through the current palette.
Horizontal OBJ mosaic uses the current width. Window masks, blending coefficients, global display enables, and color effects also remain current.

A write before the preceding line's cycle40 can affect preparation. A write after that event cannot change the prepared row.
Under this boundary policy, a host write at cycle40 is already too late.
CPU/DMA writes commit before their nominal clock batches, so a write crossing cycle40 participates in preparation.
For example, an OAM write in row0 HBlank affects row2 or later, not the already-prepared row1.

Global OBJ disable makes preparation empty. Disabling OBJ before composition hides an already-prepared row.
OBJ-window coverage is prepared regardless of its display-enable bit; composition requires both OBJ and OBJ-window enables.
Forced blank does not stop preparation in this model. Hardware restart delays remain unmodeled.

Reset starts at visible row0 rather than a hardware boot phase.
The first positive clock advance therefore prepares row0 from host setup, using mosaic phase0.
Later row0 preparation uses line227. Zero-cycle advances do not initialize or refresh the buffers.
Preparation continues with capture disabled. Capture toggles and debug snapshots do not reset prepared state.

Preparation errors are retained for the target row, not returned by CPU/DMA clock advancement.
Visible composition reports them through the existing deferred frame diagnostic.
Forced blank and global OBJ disable suppress unused sprite diagnostics. A later valid frame restores presentation.
Individual fetch timing and video-bus arbitration remain unmodeled.

### Nominal sprite work allowance

Each prepared row receives 1,210 nominal sprite-rendering cycles, or 954 when DISPCNT bit 5 (`HBlank Interval Free`) is set.
Debug snapshots apply the same allowance independently to each row, using current registers.
This is an aggregate rendering limit. It does not advance CPU or display clocks.

Objects consume work in OAM order, regardless of display priority:

- An active regular sprite costs its drawing-canvas width.
- An active affine sprite costs `10 + 2 × drawing-canvas width`.
- Double-size affine mode charges the enlarged canvas, not the source texture width.
- These active costs include inspection. Disabled, vertically excluded, and entirely horizontally offscreen entries cost two inspection cycles instead.
- A sprite that exceeds the remaining allowance prepares only a left-to-right canvas prefix. Later objects produce no samples.
- An incomplete affine setup or pixel consumes the remaining allowance. A later regular sprite cannot reuse it.
- There is no separate 32-object-per-row limit.

Transparency, occlusion, window masks, mosaic, out-of-texture affine samples, and bitmap-restricted tile numbers do not refund work.
Partially clipped sprites charge their full canvas width, including clipped columns. Hardware left-clipping savings are not modeled.
Invalid active OAM entries still produce development diagnostics after allowance exhaustion, preserving the existing validation policy.

DISPCNT bit 5 is sampled during whole-row preparation, not during later composition.
Changing it after the preceding line's cycle40 therefore cannot change that prepared row's allowance.
The reduced allowance does not implement HBlank OAM access arbitration.
These nominal costs combine documented limits with emulator-reference inspection costs; exact hardware cutoffs remain unverified.

### Mosaic snapshots and vertical counters

`MOSAIC` at `0x0400004c` is write-only. Reads return zero as an open-bus placeholder.
Byte writes merge into its 16-bit latch. The unused upper halfword at `0x0400004e` reads zero and ignores writes.
CPU and DMA halfword/word writes use the same path.

| Bits | Dimension, stored as size minus one |
| --- | --- |
| 0–3 | Background horizontal size |
| 4–7 | Background vertical size |
| 8–11 | Sprite horizontal size |
| 12–15 | Sprite vertical size |

Each dimension ranges from one to sixteen pixels. Static zero fields give ordinary 1×1 sampling.
A mid-frame write of zero preserves any existing vertical phase until the counter next resets.
BGCNT bit 6 enables mosaic for an individual background. OBJ attribute 0 bit 12 enables sprite mosaic.
The register alone does not enable a layer's mosaic effect.

Debug snapshots align background blocks to the screen origin, not tile boundaries or scroll offsets.
Their upper-left samples enter text scrolling or affine mapping before palette lookup and composition.
Captured rows instead use live vertical phase; their horizontal blocks remain screen-aligned.
This applies to Modes 0–5, including both Mode 4/5 pages, transparent indices, and transformed bitmap edges.
Window masks use the actual output coordinate, so a window can cut through a mosaic block.

Sprite snapshots use the screen's mosaic row. Prepared sprite rows subtract the latched ahead-of-display counter from the local row.
Both paths clamp a partial first block to the sprite's first row.
Flips and affine transforms follow that row selection. Vertical mosaic does not enlarge the drawing area.
Horizontal sprite mosaic latches the selected object sample after object priority resolution.
The latch updates at a block boundary, at transitions to or from non-mosaic samples, or for a lower priority number.
Transparent samples participate through their priority and mosaic flags. Each scanline starts with an empty latch.
The latch runs before window masking and alpha blending; this is not a final-frame pixelation filter.
OBJ-window coverage ignores both mosaic dimensions. Transparent window texels can still affect object metadata.

Captured rows use one four-bit vertical counter for all backgrounds and another for sprites.
The background counter follows displayed rows. The OBJ counter follows the row being prepared, one row ahead.
At each applicable line end, a counter increments by one.
If the incremented value equals the current height, the counter resets to zero. Otherwise it wraps to four bits.
The background counter resets at entry to row160. The OBJ counter resets one displayed line earlier and stays zero through line227.
At frame wrap, OBJ advances to the phase for preparing row1; row0 has already retained phase0.

Size writes preserve the current counters. Shrinking below the current phase can delay the next block until four-bit wraparound.
For example, changing height8 to height2 at phase6 produces phases7 through15 before zero.
Text backgrounds then sample `screen_y - background_counter`, followed by current scroll and tile lookup.
Affine backgrounds instead retain their origin and add `height * PB/PD` when the counter becomes zero.
After that example's delayed reset, an affine origin advances by two rows, not sixteen.

Layer enable bits, per-layer mosaic flags, forced blank, and host capture enable do not pause or reset these counters.
Mosaic-disabled layers ignore the counters. OBJ-window coverage also ignores them.
Background VRAM samples at capture; sprite OAM/VRAM samples during preparation. Palette lookup remains current at composition.
Vertical mosaic does not copy a cached row of final RGB pixels.
Snapshots, presentation, failed batched writes, and zero-cycle advances do not change counter state.
Fixed-register clock batches remain constant-time, including frame wrap.

**Timing limit:** counter updates use line boundaries, and sprite preparation samples a whole row at cycle40.
Individual sprite fetches, per-access size latching, and horizontal counter timing remain unmodeled.
The selected counter rules follow reference implementations. They have not been verified here against GBA hardware test programs.
The mosaic demo shares the tile scene's ARM program builder and writes MOSAIC during each VBlank update.

### Window masks and color effects

| Registers | Addresses | Access and stored masks |
| --- | --- | --- |
| WIN0H, WIN1H | `0x04000040..0x04000043` | Write-only, eight-bit horizontal bounds |
| WIN0V, WIN1V | `0x04000044..0x04000047` | Write-only, eight-bit vertical bounds |
| WININ, WINOUT | `0x04000048..0x0400004b` | Read/write, `0x3f3f` |
| BLDCNT | `0x04000050..0x04000051` | Read/write, `0x3fff` |
| BLDALPHA | `0x04000052..0x04000053` | Read/write, `0x1f1f` |
| BLDY | `0x04000054..0x04000055` | Write-only, `0x001f` |

Byte writes merge into the stored halfwords. CPU and DMA writes use the same register path.
Write-only reads return zero, not hardware open-bus values. Unused bytes at `0x04000056..0x04000057` read zero and ignore writes.
MOSAIC at `0x0400004c` and its unused upper halfword are described above.

DISPCNT bits 13–15 enable WIN0, WIN1, and OBJ windows.
When no window is enabled, every globally enabled layer can display and color effects are permitted.
Otherwise WIN0 takes precedence over WIN1, then OBJ window, then the outside region.
Masks are selected by region; overlapping windows do not combine their masks.
Each region independently enables BG0–BG3, OBJ, and effects. A window cannot enable a globally disabled layer.
The palette backdrop remains visible when every layer is masked out.

Bounds store the start in the high byte and the excluded end in the low byte.
Debug snapshots use wrapping eight-bit intervals. Equal bounds select an empty interval.
Captured rows instead use persistent vertical flags for WIN0 and WIN1:

- At each scanline start, matching the top edge sets the flag; matching the bottom edge clears it.
- The bottom edge wins when both bounds match the same scanline.
- With no matching edge, the flag keeps its previous value, including across frame boundaries.
- Hidden rows 160–227 run the same comparisons. VCOUNT never reaches bounds 228–255.
- WIN0V/WIN1V writes change future comparisons. They do not replay an edge from an already-entered row.
- Window enable bits and forced blank affect composition, not vertical tracking.

Moving the top behind VCOUNT does not open an inactive window.
Moving the bottom behind VCOUNT can leave an active window open until a later matching edge.
Equal unreachable bounds can retain an active flag even though the debug snapshot shows an empty interval.
A wrapping window can differ on the first frame because its top edge has not occurred yet.

Both flags start clear. Host setup before the first positive clock advance participates in the initial line-zero comparison.
After that startup step, even a write at cycle zero cannot repeat that line's comparison.
Zero-cycle advances, snapshots, and capture toggles do not update the flags.
Tracking also runs with capture disabled. Fixed-register bulk advancement remains constant-time.

Captured rows also retain horizontal comparator history for each window:

- X runs from 0 through 255, with comparisons at line cycles 0, 4, …, 1020.
- Matching the left edge sets the flag. Matching the right edge clears it; right wins when both edges match.
- Visible columns 0–239 retain their flags for later row composition. Offscreen columns still update the persistent flag.
- Flags carry across line and frame boundaries. Hidden lines, disabled windows, and forced blank do not stop tracking.
- WIN0H/WIN1H writes affect future comparisons, not already-recorded columns.

Advancing clocks consumes the half-open interval from the current cycle to the ending cycle.
A write at cycle `4*x` therefore precedes X's comparison; a write at `4*x+1` is too late.
A zero-cycle advance does not perform a comparison.
Moving an edge behind the current column can miss it and retain the old flag into the following line.
Wrapping horizontal bounds can differ on the first row because no previous line has set the flag.

HBlank starts at cycle1006, after all visible columns have been recorded.
Columns252–255 occur later, so HBlank DMA can still set or cancel an offscreen edge before the next line.
Already-captured rows remain unchanged. The horizontal history records window coverage, not pixel colors.

Region masks, enable bits, effects, palette, and background VRAM remain current-row inputs sampled at HBlank.
Sprite OAM/VRAM and OBJ-window coverage come from the preceding line's preparation.
CPU/DMA writes still commit before their nominal cycle batch. Per-access bus timing and the LCD merge pipeline remain unimplemented.
This comparator model follows a reference implementation; it has not been verified here against GBA hardware test programs.

Horizontal history is retained even with capture disabled. Capture toggles and debug snapshots do not reset it.
Bulk clock advancement processes at most three line segments, regardless of the number of crossed frames.
This preserves constant-time advancement relative to emulated cycles, with fixed 240-column history buffers.

OBJ window coverage comes from nonzero sprite texels, regardless of their palette colors or background priorities.
Window sprites contribute no color pixels. Both DISPCNT bits 12 and 15 must be set for OBJ window coverage.
Normal sprites do not occlude window coverage, and the regional OBJ display bit does not disable the window mask.

The renderer selects the two frontmost unmasked, non-transparent layers before applying effects.
Alpha blending requires the top layer to be a first target and the immediate lower layer to be a second target.
A non-target layer between them prevents blending; the renderer does not search farther back for a target.
The backdrop can be a target but cannot blend with itself. Only one OBJ color pixel participates, so OBJ-to-OBJ blending is unavailable.
Coefficients are stored as five-bit values and clamped to 16 when used.
Each RGB555 channel uses `min(31, (first*EVA + second*EVB) / 16)`, with integer division after summing.
Brightness uses `color + (31-color)*EVY/16` or `color - color*EVY/16` on selected top-layer channels.
No layer receives brightness changes before it becomes a blend target.

Semi-transparent OBJ pixels request alpha blending regardless of BLDCNT's mode and first-target selection.
They still require an eligible second target and the region's effect-enable bit.
Without a second target, a selected brightness effect can apply instead.
Green swap runs after color effects. Forced blank bypasses all composition and effects.

The effects demo reuses the tile scene and ARM instruction builder.
Its CPU writes a 112×80 WIN0 rectangle, disables effects inside it, and enables effects outside it.
BLDY and both alpha coefficients are eight. Z selects BG0/OBJ as first targets and BG1 as the second target.
Three debug words at `0x02000000` retain the update count and nine-bit horizontal/vertical scroll.
The demo updates registers during VBlank. Vertical window state still depends on edges reached in previous rows and frames.
Moving its bottom beyond row227 can keep the unchanged-color region active at the next frame's top.
The smoke test tracks this state instead of assuming a geometric rectangle on every frame.

### CPU-driven tile and background demos

The tile demo copies 1 KiB palette and OAM images with `CpuSet`, and 96 KiB video data with `CpuFastSet`.
Its ARM program configures a 4-bit foreground, an 8-bit 512×512 terrain background, and one 16×16 sprite.
Unused sprite entries are explicitly disabled.
It installs a BIOS IRQ callback and uses `VBlankIntrWait` before updating scroll registers and OAM.
Z sets the regular sprite's horizontal-flip bit. X changes its background priority from zero to one.
Q and W select affine double-size mode and write matrix zero through real ARM halfword stores.
Q uses `[181, 181, -181, 181]` for a quantized 45-degree clockwise rotation.
W halves the coefficients. With Q held, the matrix becomes `[90, 90, -90, 90]`.
The demo moves the drawing-area origin from `(112, 72)` to `(104, 64)` to preserve the center at `(120, 80)`.
Releasing Q and W restores regular mode. Z has no effect while affine mode is active.
Opposite directions cancel. Enter resets both offsets. Offsets wrap to nine bits.
Three debug words at `0x02000000` hold the update count, horizontal scroll, and vertical scroll.
The tile, effects, mosaic, raster, bitmap, and affine background demos share row capture, bounded frame execution, and keyboard handling.

The Mode 4/5 and affine raster demos reuse the affine background demo's ARM instruction builder.
The affine raster program builds 160 halfwords at `0x02001000` during each VBlank.
It rearms DMA0 to copy one halfword to BG2PB at every visible HBlank.
Sixteen positive offsets alternate with sixteen negative offsets. Z uses the base transform without distortion.
The CPU writes all assets, tables, registers, and DMA configuration. The host only supplies buttons and presents captured frames.
Their CPU copies a 512-byte palette with `CpuSet` and an 80 KiB two-page video image with `CpuFastSet`.
During each VBlank update, ARM code stores the transform and selects the page from bit 5 of its update counter.
Z overrides that selection with page 1. Three debug words at `0x02000000` hold the update count and signed pan X/Y.
The host does not use the counter as a breakpoint or write DISPCNT.

The affine background demo uses Mode 2 BG2 with a 256×256 map and four original eight-bit tiles.
Its CPU copies a 512-byte palette with `CpuSet` and a 64 KiB video image with `CpuFastSet`.
Each update stores the matrix and computes the reference point as `(map center + pan)*256 - matrix*(screen center)`.
Q uses `[181, 181, -181, 181]`; W halves the coefficients. Z clears BGCNT's overflow bit.
The CPU uses `VBlankIntrWait` before each update. Three debug words at `0x02000000` hold update count and signed pan X/Y.

The graphics demo's ARM code initializes DISPCNT and configures DMA3 to fill all 38,400 background pixels under forced blank.
DMA3 reads a fixed halfword at `0x02000010`; the CPU supplies its color and waits while DMA fills video RAM.
ARM code then polls KEYINPUT and erases/redraws the square.
It clamps movement to the screen. Opposite directions cancel, including at screen edges.
The host supplies input and renders the result; it does not write the demo's pixels or position.
The ARM program installs a ROM-side IRQ callback and enables VBlank requests in DISPSTAT and IE.
Before each redraw, it invokes `SWI 0x050000` for VBlankIntrWait.
The service discards the old BIOS flag and uses HALT while waiting for the next VBlank request.
After wake-up, IRQ dispatch calls the ROM handler to acknowledge IF and record the BIOS flag.
The service consumes that flag and restores the caller's status before ARM code redraws during VBlank.
It stores an update counter, x, y, and color at four words starting at `0x02000000`, for tests and inspection only.
The host no longer uses this counter as a breakpoint.

The first call runs background initialization under forced blank, waits for Mode 3 to become active, and primes the VBlank wait.
The host then runs between consecutive VBlank-entry events and presents the captured image from the preceding visible period.
A 200,000-machine-step limit bounds startup and frame execution, including DMA units and HALT idle intervals.
Errors retain completed steps but do not replace the output image.
Normal demo calls span one display frame and stop at the exact VBlank edge during HALT.
Other programs can still overshoot by an instruction or DMA-unit cost.
The graphics window limits presentation to approximately 59.73 Hz using the GBA clock ratio.
Host sleeps do not advance emulated time. Slow hosts do not skip emulated frames.
The original host-generated display test keeps its independent 60 Hz cap.
