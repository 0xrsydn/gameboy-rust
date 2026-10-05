# Testing and validation

Run native macOS validation from the project directory:

```sh
direnv exec . cargo fmt --all --check
direnv exec . cargo clippy --locked --all-targets -- -D warnings
direnv exec . cargo test --locked
direnv exec . cargo test --locked --release
direnv exec . cargo build --locked --release
file target/release/gameboy-rust
./target/release/gameboy-rust --smoke-test
./target/release/gameboy-rust --graphics-smoke-test
./target/release/gameboy-rust --tile-smoke-test
./target/release/gameboy-rust --affine-smoke-test
./target/release/gameboy-rust --affine-raster-smoke-test
./target/release/gameboy-rust --bitmap4-smoke-test
./target/release/gameboy-rust --bitmap5-smoke-test
./target/release/gameboy-rust --effects-smoke-test
./target/release/gameboy-rust --mosaic-smoke-test
./target/release/gameboy-rust --raster-smoke-test
./target/release/gameboy-rust --cpu-demo
./target/release/gameboy-rust --timer-demo
```

All ten demo window smoke tests need an active desktop session. Ordinary tests do not open windows.
Run the additional file-backed ROM-window test explicitly in that desktop session:

```sh
direnv exec . cargo test --locked -p gameboy-rust-desktop --test rom_cli native_rom_window -- --ignored --test-threads=1
direnv exec . cargo test --locked --release -p gameboy-rust-desktop --test rom_cli native_rom_window -- --ignored --test-threads=1
```

This opt-in test opens native windows in child processes and checks bounded presentation and STOP/CPU/video diagnostics.
Each process has a host-side timeout. It does not require or validate physical keyboard presses.
CPU-driven smoke tests supply scripted buttons; they do not validate physical keyboard events.
To test the core and demo programs without building the window dependency:

```sh
direnv exec . cargo test --locked -p gba-core -p gba-demos
```

This excludes the desktop and command-line tests.
The core's integration tests use `gba-demos` as a development dependency.

On Apple Silicon, `file` must report a Mach-O `arm64` executable.
Do not set a Linux cross-compilation target for this validation.

The [headless ROM suite runner](rom-tests.md) provides explicit checkpoint and assertion results through `--test-suite PATH.json`.
Its tests verify manifest limits, whole-suite validation, exact step boundaries, ARM/Thumb checkpoints, and register/CPSR/memory checks.
Additional tests cover DMA/IRQ accounting, HALT/STOP, diagnostic state, case isolation, repeatable JSON output, and failure exit status.
These original tests validate the runner. The [public ARM adapter](public-arm-tests.md) provides a separate passing checkpoint result for its pinned ROM.
The [public Thumb adapter](public-thumb-tests.md) uses a separate lock and r7 assertion after its return to ARM state.
The [public memory adapter](public-memory-tests.md) uses its own lock and an ARM-state r12 checkpoint.
The [public BIOS adapter](public-bios-tests.md) also passes its ARM-state r12 checkpoint in debug and release builds.
Preparation tests use only synthetic original bytes and no network:

```sh
direnv exec . python3 -B -m unittest discover -s tools -p 'test_*.py' -v
```

They verify suite selection, pinned URLs, hashes, sizes, checkpoint validation, offline copies, destination safety, and non-overwriting output.
Thumb cases also verify the return-to-ARM bridge. Command-line tests cover default ARM selection, explicit Thumb/memory/BIOS selection, and errors.
Compare/status CPU regressions verify saved flags/masks, bank switching, sequential PC/timing, User/System fallback, and atomic diagnostics.
Load-alias regressions cover widths, signed/unaligned values, immediate/register offsets, pre/post-indexing, every non-PC register bank, and address wrapping.
They verify loaded-value precedence, preserved flags, unchanged stores, nominal timing, I/O reads before device progress, deferred IRQ delivery, and atomic errors.
ARM open-bus regressions cover unused-range boundaries, PC+8 source regions, byte lanes, signed/unaligned loads, writeback, and block loads.
They check branch-target resampling, missing lookahead, skipped conditions, nominal timing, device progress, and retained host/fetch/cold-IWRAM/DMA diagnostics.
Thumb open-bus tests cover supported code regions, both instruction alignments, ROM windows, mirrors, and physical memory wrap.
They check widths, sign extension, rotation, block/stack/PC loads, aliases, modes, state preservation, and sequential/branch resampling.
Short-ROM tests require only the fetched halfword in 16-bit regions. Cold IWRAM and region-crossing tests retain explicit diagnostics.
Sequential IWRAM regressions check retained lanes across fetches, reads, writes, mirrors, and physical RAM wrap.
They cover raw bus values before load rotation/sign extension, all modes, block transfers, and isolation from other memory regions.
Known-bit tests reject incomplete words. Transaction tests verify discarded partial updates and preserved history after errors.
Control-flow tests distinguish taken/untaken branches and refill Thumb IWRAM targets even when the target equals fallthrough.
Refill tests cover ARM/Thumb BX, Thumb branches and BL, high-register PC writes, stack/block PC loads, and saved-state returns.
They check both target alignments, mirrors/wrap, captured target+2 versus later target+4, widths, sign extension, and rotation.
Original SWI/IRQ handlers verify return-time history. Failed returns, cold entry, region boundaries, DMA invalidation, and BIOS isolation retain checks.
DMA and IRQ tests check conservative invalidation; timed/untimed tests verify CPU results and nominal clock costs.
Host/fetch isolation, separate BIOS history, unsupported stores/data regions, nominal device progression, and DMA diagnostics also have checks.
The pinned public ARM ROM now reaches its completion checkpoint with r12 = 0 in debug and release builds.
The pinned public Thumb ROM also reaches its ARM-state checkpoint with r7 = 0 in both builds.
Empty-list Thumb regressions cover PC+6, all low bases and processor modes, banked stacks, both code alignments, and unaligned data.
They check one-word writes, 64-byte writeback, ROM windows, the following PC operand, nominal timing, and atomic errors.
Existing ARM empty-list and Thumb empty-load tests continue to pass. `PUSH {lr}` retains its normal link value.
The pinned public memory ROM also passes in both builds, but its video-byte assertions and mode selection have documented limits.
Separate original CPU tests check exact video-byte results across supported modes, mirrored addresses, byte lanes, and boundaries.
They require unchanged nonzero sentinels for ignored writes and unchanged adjacent halfwords for duplicated writes.
They also check forced blank, full source-value preservation, CPU halfword readback, nominal data costs, and device progress.
Halfword/word stores remain writable in object regions. No emulator behavior change was needed for these tests.
These results do not establish full BIOS, timing, graphics, save, or commercial-game compatibility.

ROM-runner tests use temporary files containing original instructions, without Nintendo assets or a desktop session.
They check file-size boundaries, bounded reads, short reads, I/O errors, paths, symlinks, and read-only loading.
CLI tests cover option order, missing/conflicting options, invalid step limits, process exit status, and reports.
Execution tests cover original ARM/Thumb code, BIOS boot, DMA/IRQ step accounting, HALT budgets, and prompt STOP results.
Unsupported instructions, truncated code, and DMA errors retain the failure state for inspection.
ROM-window session tests compare every pixel from an original CPU-written palette with expected colors.
They check keyboard-to-ROM input, focus loss, repeated STOP polling, keypad wake, HALT progress, and bounded slices.
Further checks cover frame-budget retention/reset, DMA/IRQ step accounting, video errors, and final reports.
See [cartridge execution](hardware/cartridge.md) for usage, limitations, and a generated original ROM for manual input testing.

Angle-service tests compare results with wide-integer polynomial and signed-sector references, plus floating-point accuracy checks in supported ranges.
They cover a dense ArcTan unit-interval grid, sampled full-domain and seeded inputs, axes, quadrant boundaries, signed extremes, and scale invariance.
Other tests verify all caller flag combinations, User/System masks, non-result registers, stack bounds, repeated calls, diagnostics, and DMA/IRQ progress.
Huffman tests use an independent tree serializer and path encoder, with expected output taken directly from original symbols.
They cover every byte/nibble value, maximum tree size and offsets, deep paths across input-word boundaries, and seeded variable-length paths.
Other tests check 24-bit byte lengths, malformed trees, output packing, truncation, partial failures, source regions, and DMA/IRQ behavior.
Differential-filter tests round-trip original samples through an independent difference encoder, including all byte and halfword sample values.
They cover seeded inputs, 24-bit byte lengths, full-register accumulation wrap, output widths, source regions, and caller status.
Malformed headers, truncated inputs, partial output, video-bus behavior, DMA progress, and masked IRQ delivery have separate regression tests.
BitUnPack tests compare all supported width pairs and byte values with an independent bit-by-bit reference.
They cover offsets and the zero-data flag, seeded inputs, maximum source length, source alignment, and ROM descriptors.
Other tests check output boundaries, video-memory word writes, malformed/truncated inputs, partial failures, caller status, and DMA/IRQ behavior.
Run-length tests cover all 256 block-control values and repeated byte values, plus seeded mixed streams and 65,536-byte output.
They verify ARM/Thumb callers, User/System status restoration, stack bounds, source regions, and exact output boundaries.
Other tests check malformed/truncated input, halfword buffering, normal video-bus writes, DMA progress, and masked IRQ delivery.
Sprite allowance tests cover regular/affine costs, doubled canvases, partial prefixes, inactive inspection, clipping policy, and OAM-order allocation.
They check transparent, occluded, out-of-texture, and bitmap-restricted work, plus independent row allowances and preserved diagnostics.
DISPCNT bit5 boundary writes and HBlank DMA verify preparation-time sampling. Static captures match immediate snapshots.
Budget unit tests compare every allowance and supported canvas width with independent cycle consumption, plus seeded mixed workloads.
Sprite preparation tests check cycle39/40/41 boundaries, buffered attributes and indices, late palette/effect lookup, and affine matrices.
They cover OBJ windows, horizontal mosaic, global enable, forced blank, HBlank DMA, row0 preparation, and deferred diagnostic recovery.
Large capture-disabled batches and capture toggles preserve prepared rows. Failed CPU steps and zero-cycle advances do not prepare rows.
An independent line-event model checks preparation scheduling, including maximum-length clock batches.
Horizontal window tests cover all 65,536 bound combinations, partial-cycle boundaries, missed edges, and right-edge precedence.
They compare random register/clock sequences with independent dot iteration and check maximum-length batches.
Integration tests cover hidden columns, late HBlank DMA, independent windows, CPU stores crossing edges, and capture independence.
They also preserve the distinction between window history and row-sampled pixel colors.
Vertical mosaic tests cover size growth, size shrinkage, four-bit wraparound, byte writes, independent counters, and frame resets.
They check affine origin updates, sprite partial blocks, OBJ-window exclusion, live memory sampling, DMA ordering, and capture independence.
All counter values and heights are compared with independent iteration across every visible line interval.
Random clock batches, maximum-length batches, and failed-write atomicity also have regression coverage.
Vertical window tests cover missed edges, equal bounds, hidden rows, frame wrap, byte/word writes, and independent WIN0/WIN1 state.
They check DMA ordering, enable/forced-blank behavior, snapshot independence, capture toggles, and failed-write atomicity.
All 65,536 vertical bound combinations are checked against an independent edge-iteration model across two frames.
Additional tests compare random clock batches and a maximum-length batch against individual line transitions.
Affine tracking tests cover cumulative coefficients, partial reference writes, per-axis reloads, mode/enable gates, and frame reloads.
They also check mosaic blocks, window masking, forced blank, capture-independent tracking, and failed-write atomicity.
A deterministic 500-case test compares batched affine tracking with an independent line-boundary model.
The affine raster demo tests verify DMA tables, all rotation/zoom/bypass combinations, negative panning, reset, and every pixel.
Capture tests cover exact HBlank/VBlank boundaries, partial startup frames, repeated enable, disable, large batches, and zero-cycle advances.
They check immutable completed images, deferred rendering errors, recovery, and equivalence with single-cycle clock advancement.
Mid-frame tests change palettes, video RAM, OAM, bitmap pages, scroll, windows, mosaic, brightness, forced blank, and green swap.
Static captures match snapshots across all six modes with sprites and effects.
Machine tests check HBlank/VBlank DMA ordering, HALT progress, VCOUNT polling, failed instructions, and the instruction-boundary timing policy.
The CPU raster demo tests verify table copying, DMA rearming, phase wrapping, opposite keys, reset, and every displayed pixel.
Mosaic tests cover every dimension, both bitmap pages, text flips and scrolling, affine transforms, transparency, and independent enable flags.
Sprite tests check screen-aligned blocks, partial first blocks, wrapping, regular/affine sampling, and double-size drawing areas.
They also check horizontal latch transitions, transparent priority updates, unchanged OBJ-window coverage, window edges, and alpha blending.
CPU tests cover all sixteen sizes, size cycling, scrolling, rotation/zoom, independent bypass controls, reset, and DMA writes.
Window tests cover bounds, wrapping, overlap precedence, layer masks, effect gating, OBJ coverage, and regular/affine window sprites.
Color tests cover five-bit rounding, saturation, coefficient clamping, backdrop selection, immediate targets, and semi-transparent OBJ precedence.
They check all display modes, green swap, forced blank, register widths, DMA writes, and CPU-driven brightness frames.
The native effects smoke test verifies every pixel across brightness increase, decrease, alpha blending, and window movement.
A CPU-demo regression checks retained vertical state when the moving bottom reaches the unreachable bound228.
Mode 4/5 tests cover both page boundaries, byte ordering, hidden-page writes, palette changes, transparency, and opaque black.
They check source dimensions, affine transforms against wide-integer references, clipping, sprite restrictions, and memory write rules.
Both CPU bitmap demos run across two automatic page transitions. Tests compare every displayed pixel and verify VBlank synchronization.
Additional tests cover panning, rotation, zoom, forced page selection, opposite directions, and reset.
Affine background tests compare all map sizes and both backgrounds against a separate wide-integer sampling reference.
They cover signed origins, reference masking, register access widths, clipping, wrapping, tile255, and character base3.
Other tests cover Mode 1 text/affine mixing, independent BG2/BG3 state, priorities, sprites, and Mode 3 transforms.
The CPU background demo test checks negative panning, opposite keys, reset, every control combination, and every output pixel.
Snapshot tests verify the debug API's whole-frame register policy. Capture tests separately verify the documented HBlank row-sampling policy.
Affine tests compare identity matrices against regular sprites across every shape, size, color depth, and tile layout.
A separate floating-point reference checks rotation, reflection, shear, fractional scaling, negative rounding, and double-size drawing areas.
Tests also cover all 32 matrix groups, shared matrices, coordinate wrapping, singular/extreme values, priorities, and bitmap tile restrictions.
The CPU demo test checks all Q/W/Z/X combinations, matrix writes, preserved screen center, and return to regular mode.
Sprite tests cover OAM mirrors, ignored byte writes, DMA transfers, every shape/size, and both palette depths.
They check 1D/2D layouts, address wrapping, odd eight-bit bases, flips, clipping, opaque black, and priority selection.
Other tests check Mode 3 tile restrictions, green swap, disabled objects, and unchanged output after diagnostics.
The CPU demo test checks sprite controls, OAM attributes, disabled unused objects, and every output pixel.
Tile tests cover both color depths, palette banks, flips, all map sizes, scrolling, transparency, priorities, and register masks.
They check errors for unsupported features and out-of-range fetches without changing the output image.
The CPU tile demo test verifies asset copies, every pixel, wrapping, opposite directions, reset, and VBlank synchronization.
RegisterRamReset tests check full memory regions and all memory-flag combinations from ARM and Thumb.
They verify forced blank, reserved flag bits, caller preservation, protected BIOS RAM, and serial/sound diagnostics.
Rendering tests check write-only display state, both identity matrices, and the effect of zeroed sprite attributes.
Device tests check DMA address/count latches, stopped timer counters, cleared reloads, and IRQ acknowledgement.
Other checks cover partial clears, preserved input/device state, and callers whose own RAM code is erased.
SoftReset tests check every restart flag byte from ARM and Thumb, exact RAM clearing, and preserved memory.
They seed CPU banks and verify stack pointers, link registers, saved status, caller modes, and restart status.
Other tests execute RAM restart code, call BIOS services again, and restart from code inside the erased region.
Device tests check retained I/O state, advancing timers/DMA, pending IRQs, and instruction-by-instruction RAM clearing.
BIOS affine tests check all 256 angle phases against independent trigonometric and wide-integer references.
They cover signed scales, origin wraparound, rounding, ignored angle bits, strides, alignment, and partial failures.
Live rendering tests apply generated matrices to background registers and sprite attribute memory.
Other checks cover ARM/Thumb calls, register/status preservation, stack bounds, DMA, timers, and deferred IRQ delivery.
Image-builder tests verify multiple literal pools, branch fixups, and reserved table boundaries.
BIOS arithmetic tests cover signed boundaries, division overflow/zero, all sign combinations, and seeded integer inputs.
Square-root tests check exact integer bounds around perfect squares and across unsigned inputs.
LZ77 tests compare emitted-code execution against independent token expansion, including 4,096-byte distances and overlapping runs.
They also check byte/halfword output, exact buffer boundaries, stack limits, malformed headers/references, and truncated input.
BIOS tests cover ARM/Thumb service decoding, register/status restoration, default stacks, unsupported calls, and IRQ dispatch.
Copy tests cover widths, fill, zero count, reserved bits, alignment, overlap, FastSet rounding, protected sources, and partial failures.
Wait tests inject an IRQ at 100 different machine-step positions to check the flag-check/HALT transition.
Other wait tests cover discarded flags, unrelated IRQs, masked/User-mode callers, and callbacks that omit BIOS flag updates.
HALT tests distinguish wake-up from IRQ delivery, including IME/CPSR masking and ARM/Thumb resume behavior.
Idle batches are checked against a cycle-by-cycle reference with independent and cascaded timers.
Tests cover display/DMA wake sources, DMA priority while halted, BIOS-only writes, power-state validation, and frame limits.
STOP tests check zero-cycle idle steps, frozen DMA/timers/display, retained timer phases and captured frames, and live keypad wake.
They distinguish wake-up from IF latching and test stale requests, interrupt masks, already-held keys, and normal resume ordering.
ARM/Thumb BIOS Stop tests verify caller preservation. Frame-runner tests check prompt stopped results and unchanged output images.
The graphics test verifies BIOS VBlank waiting, one IRQ callback per update, and status restoration before each redraw.
DMA tests cover both widths, all supported address modes, count limits, register masks, enable latches, and repeated transfers.
Tests verify channel priority/preemption, CPU pausing, display/timer progress, IRQ acknowledgement/return, and WAITCNT costs.
Diagnostic tests check failed-unit isolation, retained earlier transfers, unsupported modes, and CPU block-store atomicity.
Graphics startup tests verify that DMA3, not CPU stores or host writes, fills every background pixel.
Display tests cover exact scanline boundaries, line-227 VBlank behavior, all 228 HBlank events, and read-only status fields.
Bulk display advancement is checked against a cycle-by-cycle reference, including invalid comparison values and large batches.
Original IRQ-handler tests cover VBlank, HBlank, and VCount-match delivery, acknowledgement, and return.
Frame tests verify consecutive VBlank events, step limits, error handling, and independence from the demo's RAM counter.
Graphics tests check CPU-generated pixels, movement, edge clamping, opposite directions, colors, and reset behavior.
Video tests cover mirrors, byte-write rules, RGB555 layout, backdrop, forced blank, green swap, and unsupported modes.
Input tests cover all 1,024 button combinations, read-only KEYINPUT behavior, keyboard mapping, and focus loss.
Keypad tests compare every button-state/selection pair with independent per-button OR/AND logic.
They check all KEYCNT values, byte merging, complete-word sampling, empty masks, and repeated request behavior.
Integration tests cover CPU/DMA writes, interrupt masks, HALT wake-up, ARM/Thumb IRQ return, and original BIOS IntrWait.
Failed-store tests check register, IRQ-history, CPU, and clock preservation. BIOS reset tests verify retained KEYCNT state.
The bounded graphics frame runner is checked for instruction errors and timeouts.
Timing tests cover every ROM wait-state setting, access width, ROM window, and 128 KiB boundary handling.
Instruction timing tests cover arithmetic, shifts, branches, transfers, block operations, multiply, and exception returns.
Timed and untimed execution are compared across all 65,536 Thumb encodings with fixed initial registers.
Timer/IRQ tests verify that changing WAITCNT changes when an interrupt is sampled.
Timer tests cover all prescalers, reload changes, start/stop behavior, byte writes, large clock advances, and cascades.
Bulk timer advancement is compared against an independent cycle-by-cycle reference for each prescaler and cascade configuration.
Machine tests cover ARM/Thumb IRQ return, pending-request gating, acknowledgement, and the complete timer demo.
Failed I/O block transfers are checked for partial register writes, hidden reload changes, and unintended IF acknowledgement.
Status tests cover all modes, flag combinations, interrupt masks, field selection, privilege rules, and bank isolation.
Exception tests cover vectors, ARM/Thumb return addresses, nested interrupts, mask priority, and return-state restoration.
User-bank transfers and failed returns are checked for unintended changes to active and hidden registers.
Integration tests execute original vector handlers and check optional BIOS mapping boundaries and write protection.
Original BIOS-access regressions check retained ARM PC+8 and aligned Thumb PC+4 words for ARM/Thumb callers.
They cover all processor modes, load widths, lanes, rotation, both Thumb code alignments, and distinct halfwords.
They verify direct reads while executing inside BIOS, re-entry, SWI/IRQ returns, host-inspection isolation, and original-boot provenance.
Thumb tests cover sequential snapshots, instruction-state transitions, SWI entry, and RAM-backed POP exits.
Unknown history, last valid fetch words, missing lookahead, and diagnostic-step retention have explicit checks.
Unknown Thumb IWRAM history remains diagnostic. Separate tests cover sequential/refill history, general Thumb snapshots, and retained BIOS history.
Protected loads keep normal nominal costs and advance devices only through machine stepping.
Firmware readback regressions separately verify documented boot, SoftReset, returning SWI, IRQ callback, and IRQ return words.
They execute ARM/Thumb loads across widths and lanes, check caller state, and trace actual exits to their image data.
Interrupt waits replace callback readback with SWI readback on return. Builder tests check branch/literal relocation around exit data.
A modified original image verifies that the bus retains supplied data rather than forcing compatibility constants.
The pinned public BIOS ROM passes unchanged; this is a bounded protected-read result, not complete BIOS conformance.
Display tests cover color conversion, framebuffer bounds, cursor movement, pause behavior, and animation wraparound.
Input regression tests check focused arrow keys, focused Space presses, and ignored input when unfocused.
Command-line tests cover mode selection and invalid arguments.
Condition tests cover all 15 defined conditions against all 16 flag combinations.
Arithmetic tests check all 4,096 immediate encodings against nine boundary operands for `ADDS`, `SUBS`, and `CMP`.
Arithmetic-with-carry tests use wider signed and unsigned integers as an independent reference.
Shifter tests compare all 256 register-shift amounts against a bit-at-a-time reference.
Instruction tests cover all data-processing opcodes, addressing modes, PC offsets, signed loads, and multiply.
Block-transfer tests cover all four addressing modes, base-register overlap, empty lists, alignment, and failure handling.
Swap tests cover byte and word transfers, register aliases, and failed writes to cartridge ROM.
Thumb tests cover arithmetic, memory, stack operations, branch offsets, high registers, and ARM/Thumb switching.
The decoder test steps all 65,536 Thumb encodings with fixed initial registers and checks for panics or partial CPU changes on errors.
This decoder test checks robustness, not hardware compatibility for every encoding or register value.
Other tests check flag preservation, skipped instructions, memory errors, and the complete mixed ARM/Thumb demo.
These tests do not replace validation against GBA hardware or public hardware test programs.
