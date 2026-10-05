# Display timing and scanline capture

## Display timing, status, and interrupts

The display clock runs from the same supplied cycles as the timers:

- 1,232 cycles per scanline, with 228 lines numbered 0–227.
- 160 visible lines and 280,896 cycles per complete frame.
- HBlank status starts at cycle 1,006 of every line, not at the last visible pixel's cycle 960.
- VBlank starts at line 160. Its status flag remains set through line 226 and clears at line 227.
- HBlank status and IRQ events occur on all lines, including hidden lines.
- Forced blank changes pixel output but does not stop or reset the display clock.

`DISPSTAT` at `0x04000004` contains read-only VBlank, HBlank, and VCount-match flags in bits 0–2.
Bits 3–5 enable the corresponding interrupt sources. Bits 8–15 select the comparison scanline.
Bits 6–7 are unused on GBA. The writable mask is `0xff38`.
`VCOUNT` at `0x04000006` reports the current line, 0–227. Writes are ignored.
Byte, halfword, and combined word accesses are supported. I/O registers are not mirrored.

VBlank, HBlank, and VCount-match events latch IF bits 0, 1, and 2 when their local enables are set.
IE, IME, and CPSR.I gate CPU delivery, not those pending flags.
Acknowledging IF during an active blank period does not replay the old event.
Changing the comparison value recomputes the match flag immediately.
A false-to-true match can latch its enabled IRQ; enabling an IRQ with its flag already set does not create an edge.
Comparison values 228–255 never match.

`Memory::display_position()` reports the scanline, cycle within the line, completed frames, and VBlank-entry count.
The clock starts at line zero, cycle zero, with comparison zero matched and no pending IRQ.
Both event counters wrap as 64-bit values. Bulk advances account for every crossed event, including multiple frames.
Display status is calculated at the resulting position. CPU IRQ delivery occurs between instructions, after ready DMA work.
Hardware IRQ synchronization delays and display bus contention are not modeled.

`Machine::run_until_vblank(max_steps)` executes through the next line-160 entry, even if already in VBlank.
It returns at the first instruction, DMA-unit, or HALT-idle boundary after that event and reports the number of machine steps.
A stopped idle step returns `FrameRunError::Stopped` because STOP freezes the display clock.
STOP retains the current display phase and captured frame; elapsed host time does not create emulated scanlines.
DMA requested by that VBlank event can still be pending; this method does not wait for a redraw or DMA completion.
A zero or exhausted step limit returns `FrameRunError::StepLimit`.
CPU and DMA diagnostics return `FrameRunError::Cpu` and `FrameRunError::Dma`, respectively.
Completed steps remain committed on error. The method does not inspect demo RAM.
When row capture is enabled, the clock advances within those steps also render visible rows.

## Scanline frame capture

`Memory::set_scanline_rendering(true)` enables row capture without changing clocks or hardware registers.
Capture is disabled by default for CPU-only use. All CPU-driven demo frame runners enable it automatically.
Repeatedly enabling capture preserves progress. Disabling it drops captured buffers and availability state.

For each visible line, the renderer composes the row at HBlank entry, cycle 1,006.
It uses current scrolling, background VRAM, palette, and effect registers, together with previously prepared sprite samples.
Affine backgrounds use internal scanline origins. Rectangular windows combine vertical flags with retained horizontal comparator history.
Vertical mosaic sampling uses separate background and sprite counters.
The completed row is retained. Later writes cannot recolor or move it.
At entry to line 160, the completed image becomes available for presentation.
This occurs before the machine services DMA requested by that VBlank edge.
HBlank DMA similarly runs after row capture. Palette and background writes can affect the following row.
OAM and sprite-tile writes wait for a later preparation event; the following row is already prepared.

`Memory::present_frame(&mut frame)` copies the latest completed captured image without advancing time or consuming the image.
It returns `Ok(false)` when capture is disabled or no complete frame is available.
`Memory::captured_vblank()` identifies the latest fully captured frame by its VBlank-entry counter, including failed frames.
Before publication, presentation keeps returning the previous complete frame.
Capture started after row zero's HBlank discards that partial frame and waits for the next full visible frame.
Starting before row zero's HBlank can capture the current frame.

Rendering diagnostics do not turn successful CPU instructions or DMA units into failures.
The capture records the first rendering error in the frame and reports it from `present_frame` after VBlank publication.
Errors and unavailable frames leave the caller's output unchanged. A later valid frame restores normal presentation.
`Memory::render_frame` remains an independent debug snapshot API. It ignores row history and uses current state for the entire screen.

With capture enabled, clock advancement splits at HBlank entries, scanline starts, and sprite preparation events.
Large clock batches still account for each crossed row, but do not execute CPU code or service pending DMA themselves.
Unserviced DMA requests still coalesce. Rendering costs host time, not additional emulated cycles.
Capture uses two 240×160 pixel buffers. Sprite preparation separately retains two rows of indices, metadata, and coverage.
With capture disabled, a clock batch prepares at most the latest two eligible sprite rows.
The bulk clock path remains constant-time relative to the supplied cycle count.

**Timing limits:** graphics data is sampled once per row, not fetched pixel by pixel.
Horizontal window comparisons are the exception: their history records four-cycle column events.
CPU instructions and DMA writes commit before their nominal cycle batch advances.
A palette or background-data write in an instruction that crosses HBlank therefore affects the entire captured row.
Sprite preparation similarly uses writes committed before its cycle40 boundary.
Window-bound writes affect only comparator events that have not yet run.
There is no sub-instruction arbitration or active-display bus contention.
Sprite preparation samples a whole row at once; it does not reproduce individual fetch timing.
Affine origins advance at line boundaries, using the current coefficients rather than per-pixel register timing.
Vertical mosaic counters advance at line ends. Horizontal mosaic still uses screen-derived block boundaries.
Horizontal window history follows the four-cycle comparator model described in [video.md](video.md), not a complete LCD pixel pipeline.
Vertical window flags update at scanline starts, independently of row capture.
These rules support raster palette, page, scroll, window, and effect changes, but not every hardware raster effect.

The raster demo copies a 256-halfword original color table to `0x02001000` with BIOS `CpuSet`.
During each VBlank, its ARM code polls input, resets the row-zero color, and rearms DMA0.
DMA0 copies one halfword to palette entry zero at each visible HBlank, with repeat and fixed destination enabled.
The source starts at the next row's color. The CPU waits through the visible frame using `VBlankIntrWait` and HALT.
Two debug words at `0x02000000` contain the update count and five-bit color-band phase.
The host neither writes palette colors nor uses those debug words to drive execution.
