# Architecture and file map

The repository is a Cargo workspace with three crates:

- `crates/gba-core`: the platform-independent GBA emulator. It has no dependencies and must not depend on window, audio, or file-system libraries.
- `crates/gba-demos`: original ARM test programs and their frame runners. Depends on `gba-core`.
- `crates/desktop`: the minifb window frontend and the `gameboy-rust` executable. Depends on both.
  Serde and serde_json handle host-side test manifests and reports; neither dependency enters the core.

`gba-core` uses `gba-demos` as a development dependency, because its integration tests run the demo programs.

## Files

| Path | Purpose |
| --- | --- |
| `flake.nix`, `flake.lock` | Pinned Rust/Python development environment |
| `tools/prepare_gba_tests.py`, `tools/gba-tests-{arm,thumb}.lock.json` | Hash-verified public ARM/Thumb source/ROM preparation and suite generation |
| `tools/test_prepare_gba_tests.py` | Offline adapter tests using original synthetic data |
| `.envrc` | Automatic environment loading through direnv |
| `AGENTS.md` | Short orientation for coding agents: crate map, commands, and where docs live |
| `docs/` | Project documentation; `docs/hardware/` holds per-subsystem behavior notes |
| `crates/gba-core/src/cpu.rs` | Registers, flags, instruction-set state, and stepping |
| `crates/gba-core/src/cpu/arm.rs` | ARM decoding, data processing, transfers, branches, and multiply |
| `crates/gba-core/src/cpu/alu.rs` | Shared arithmetic and barrel shifter |
| `crates/gba-core/src/cpu/transfer.rs` | Shared ARM/Thumb block transfers and ARM swaps |
| `crates/gba-core/src/cpu/thumb.rs` | Thumb decoding and execution |
| `crates/gba-core/src/cpu/status.rs` | Processor modes, register banks, and status transfers |
| `crates/gba-core/src/cpu/exception.rs` | Reset state, exception entry, and interrupt sampling |
| `crates/gba-core/src/cpu/load_alias_tests.rs` | Original single-load alias regressions for indexing, banks, widths, alignment, I/O, timing, and diagnostics |
| `crates/gba-core/src/cpu/compare_psr_tests.rs` | Original Rd=15 test/compare status restoration, flags, bank, and timing regressions |
| `crates/gba-core/src/cpu/status_tests.rs`, `crates/gba-core/src/cpu/exception_tests.rs` | Status, banking, exception, and return tests |
| `crates/gba-core/src/cpu/thumb_tests.rs` | Thumb instructions, state switching, and decoder checks |
| `crates/gba-core/src/cpu/thumb_empty_tests.rs` | Original empty-list stored-PC regressions for banks, alignment, writeback, timing, and diagnostics |
| `crates/gba-core/src/cpu/transfer_tests.rs` | Stack, addressing, overlap, empty-list, and swap tests |
| `crates/gba-core/src/cpu/tests.rs` | Condition truth tables and immediate arithmetic tests |
| `crates/gba-core/src/cpu/instruction_tests.rs` | Register operations, transfers, branches, and edge cases |
| `crates/gba-core/src/memory.rs` | Memory mapping, I/O routing, write validation, and device clock |
| `crates/gba-core/src/io.rs` | I/O registers, timers, HALT/STOP wake-up, next-event bounds, and IRQ latches |
| `crates/gba-core/src/display.rs` | Display clock, scanline status, comparison edges, and display IRQ events |
| `crates/gba-core/src/dma.rs` | DMA registers, internal pointers, trigger state, priority, and completion IRQs |
| `crates/gba-core/src/machine.rs` | Timed CPU/DMA/device stepping and IRQ delivery |
| `crates/gba-core/src/timing.rs` | Bus widths, wait-state costs, and timing breakdowns |
| `crates/gba-core/src/cpu/timing.rs` | ARM/Thumb cycle summaries and data-access accounting |
| `crates/gba-core/src/cpu/timing_tests.rs` | Instruction timing and semantic-equivalence checks |
| `crates/gba-demos/src/timer_demo.rs` | Original timer-configuration program and IRQ handler |
| `Cargo.toml` | Workspace members and shared package settings |
| `crates/gba-core/src/lib.rs` | Core modules |
| `crates/gba-demos/src/lib.rs` | Demo modules, original instruction/exception bytes, and raw input-test ROM |
| `crates/gba-core/src/bios.rs` | Original ARM BIOS image builder, minimal boot, IRQ dispatch, waits, and memory services |
| `crates/gba-core/src/bios/reset.rs` | Emitted ARM SoftReset and selective RegisterRamReset; CPU banks, RAM clearing, and supported device-register resets |
| `crates/gba-core/src/bios/arithmetic.rs` | Emitted ARM division and integer-square-root routines |
| `crates/gba-core/src/bios/affine.rs` | Emitted ARM background/sprite matrix services, range checks, and generated sine table |
| `crates/gba-core/src/bios/angles.rs` | Emitted ARM ArcTan polynomial, ArcTan2 ratio/quadrant handling, and input validation |
| `crates/gba-core/src/bios/lz77.rs` | Emitted ARM LZ77 decoder with byte and halfword output |
| `crates/gba-core/src/bios/run_length.rs` | Emitted ARM run-length decoder, header validation, and block bounds |
| `crates/gba-core/src/bios/bit_unpack.rs` | Emitted ARM packed-unit expansion, offsets, word stores, and validation |
| `crates/gba-core/src/bios/decompression.rs` | Shared emitted byte/halfword output routine for decompression and byte differential filters |
| `crates/gba-core/src/bios/differential.rs` | Emitted ARM byte/halfword differential filters, modular accumulation, and validation |
| `crates/gba-core/src/bios/huffman.rs` | Emitted ARM Huffman tree traversal, symbol packing, word output, and validation |
| `crates/gba-core/src/video.rs` | Presentation buffer, RGB555 conversion, and Mode 0–5 composition |
| `crates/gba-core/src/video/affine.rs` | Programmed registers, internal scanline origins, and tiled/bitmap sampling |
| `crates/gba-core/src/video/windows.rs` | Persistent vertical window edge flags and constant-time clock advancement |
| `crates/gba-core/src/video/windows/horizontal.rs` | Horizontal comparator flags, per-column history, and bounded bulk advancement |
| `crates/gba-core/src/video/mosaic.rs` | Live vertical mosaic phase and constant-time counter transitions |
| `crates/gba-core/src/video/capture.rs` | Drawing/completed frame buffers, row completeness, and deferred render diagnostics |
| `crates/gba-core/src/video/sprites.rs` | Sprite index preparation, priority metadata, mosaic, late palette lookup, and composition |
| `crates/gba-core/src/video/sprites/pipeline.rs` | Two prepared row buffers, cycle40 events, and bounded capture-disabled advancement |
| `crates/gba-core/src/video/sprites/budget.rs` | Nominal per-row work allowance, partial canvas prefixes, and independent arithmetic tests |
| `crates/gba-core/src/input.rs` | Platform-independent GBA button state |
| `crates/gba-core/src/input/keypad.rs` | KEYCNT mask, OR/AND matching, and keypad IRQ sampling history |
| `crates/gba-demos/src/graphics_demo.rs` | Original ARM bitmap program and shared bounded frame runner |
| `crates/gba-demos/src/tile_demo.rs` | Original ARM tile/sprite program, palettes, tiles, maps, and OAM image |
| `crates/gba-demos/src/affine_demo.rs` | Shared ARM background program builder and Mode 2 assets |
| `crates/gba-demos/src/affine_raster_demo.rs` | CPU-driven HBlank DMA affine distortion demo |
| `crates/gba-demos/src/bitmap_demo.rs` | Mode 4/5 demo runners, palettes, and original two-page images |
| `crates/gba-demos/src/effects_demo.rs` | CPU-driven window and color-effects demo runner |
| `crates/gba-demos/src/mosaic_demo.rs` | CPU-driven background/sprite mosaic demo runner |
| `crates/gba-demos/src/raster_demo.rs` | Original ARM HBlank DMA raster program and color table |
| `crates/gba-core/src/video/effects.rs` | Region masks and RGB555 alpha/brightness arithmetic |
| `crates/desktop/src/desktop.rs` | Native window, display test, and keyboard controls |
| `crates/desktop/src/desktop/affine.rs` | Affine background window and smoke-test pixel checks |
| `crates/desktop/src/desktop/affine_raster.rs` | Affine raster window and every-pixel smoke checks |
| `crates/desktop/src/desktop/bitmap.rs` | Mode 4/5 windows and page-flipping smoke-test pixel checks |
| `crates/desktop/src/desktop/effects.rs` | Window/effects demo and independent smoke-test pixel checks |
| `crates/desktop/src/desktop/mosaic.rs` | Mosaic window and 128-frame independent pixel checks |
| `crates/desktop/src/desktop/raster.rs` | Raster window and scanline color-band smoke test |
| `crates/desktop/src/main.rs` | Command-line modes, error exit status, and fixed-length CPU demo |
| `crates/desktop/src/cartridge.rs` | Read-only ROM files, mode/limit checks, bounded terminal execution, and shared state reports |
| `crates/desktop/src/cartridge/suite.rs` | Strict JSON manifests, bounded checkpoint execution, assertions, and structured reports |
| `crates/desktop/src/cartridge/suite/tests.rs` | Manifest limits, checkpoint boundaries, memory/register checks, and diagnostic accounting |
| `crates/desktop/tests/rom_suite_cli.rs` | File-backed suite execution, case isolation, output schema, determinism, and exit status |
| `crates/desktop/examples/write_test_suite.rs` | Original ARM/Thumb/BIOS fixture generator with a local JSON suite |
| `crates/desktop/src/cartridge/window.rs` | ROM-window session, bounded slices, capture, STOP/input handling, and progress limits |
| `crates/desktop/src/cartridge/window/tests.rs` | Original ROM pixels, keyboard mapping, STOP wake, HALT, frame budgets, and diagnostics |
| `crates/desktop/src/desktop/rom.rs` | Main-thread ROM window, input polling, presentation pacing, and frame-limited exit |
| `crates/desktop/examples/write_rom_demo.rs` | Non-overwriting export of the original input-test ROM |
| `crates/desktop/src/cartridge/tests.rs` | ROM options, bounded readers, instruction/DMA/IRQ budgets, HALT/STOP, and diagnostic reports |
| `crates/desktop/tests/rom_cli.rs` | File loading and exit-status checks; opt-in native ROM-window integration test |
| `crates/gba-core/tests/core.rs` | CPU integration tests and the complete demo |
| `crates/gba-core/tests/memory.rs` | Memory widths, alignment, errors, and mirrors |
| `crates/gba-core/tests/exceptions.rs` | Exception demo, BIOS mapping, and reset-vector execution |
| `crates/gba-core/tests/timers.rs` | Timer rules, interrupt registers, and a cycle-by-cycle reference |
| `crates/gba-core/tests/machine.rs` | Device IRQ entry/return, clock policy, and I/O failure atomicity |
| `crates/gba-core/tests/timing.rs` | WAITCNT fields, bus costs, and timer/IRQ timing integration |
| `crates/gba-core/tests/keypad.rs` | Keypad register widths, IRQ sampling, CPU/DMA ordering, HALT wake-up, diagnostics, and BIOS IntrWait |
| `crates/gba-core/tests/graphics.rs` | Video memory, rendering, KEYINPUT, and CPU-driven graphics integration |
| `crates/gba-core/tests/tiles.rs` | Mode 0 registers, maps, palettes, composition, diagnostics, and CPU tile/sprite demo |
| `crates/gba-core/tests/affine_backgrounds.rs` | Affine registers, maps, transforms, bitmap sampling, composition, and CPU demo |
| `crates/gba-core/tests/affine_tracking.rs` | Internal origins, mid-frame writes, reloads, mosaic, DMA, and affine raster demo |
| `crates/gba-core/tests/bitmap_modes.rs` | Mode 4/5 pages, formats, transforms, transparency, sprites, and CPU demos |
| `crates/gba-core/tests/effects.rs`, `crates/gba-core/tests/effects/` | Window masks, color arithmetic, target selection, OBJ effects, DMA, and CPU demo |
| `crates/gba-core/tests/window_tracking.rs`, `crates/gba-core/tests/window_tracking/` | Vertical/horizontal edges, mid-line writes, hidden positions, DMA, masks, and capture independence |
| `crates/gba-core/tests/mosaic.rs`, `crates/gba-core/tests/mosaic/` | Mosaic sizes, sampling, transparent metadata, windows, DMA, and CPU demo |
| `crates/gba-core/tests/mosaic_tracking.rs` | Size changes, counter wrap, text/affine/OBJ sampling, DMA, and frame resets |
| `crates/gba-core/tests/scanlines.rs`, `crates/gba-core/tests/scanlines/` | Capture boundaries, mid-frame changes, DMA ordering, diagnostics, and raster program |
| `crates/gba-core/tests/sprites.rs` | OAM, DMA, sprite sizes, mapping, palettes, flips, clipping, priorities, and diagnostics |
| `crates/gba-core/tests/sprite_pipeline.rs` | Preparation boundaries, OAM/VRAM history, late palette/effects, DMA, row0, and diagnostics |
| `crates/gba-core/tests/sprite_budget.rs` | Regular/affine costs, clipping policy, truncation, inspection, bit5 sampling, DMA, and diagnostics |
| `crates/gba-core/tests/sprites/affine.rs` | Affine matrices, drawing areas, signed sampling, shared groups, and floating-point reference tests |
| `crates/gba-core/tests/display.rs` | Display boundaries, register masks, IRQ handlers, and VBlank frame execution |
| `crates/gba-core/tests/dma.rs` | DMA widths, latches, priority, triggers, IRQs, timing, errors, and CPU integration |
| `crates/gba-core/tests/stop.rs` | STOP clock gating, live keypad wake, retained DMA/device phases, frame-runner results, and access rules |
| `crates/gba-core/tests/bios/stop.rs` | ARM/Thumb Stop calls, caller preservation, keypad wake, and stopped frame running |
| `crates/gba-core/tests/halt.rs` | HALT wake masks, idle timing, BIOS-only power writes, DMA progress, and validation |
| `crates/gba-core/tests/bios.rs` | ARM/Thumb service calls, copy/fill boundaries, wait races, callback contracts, and boot |
| `crates/gba-core/tests/bios/ram_reset.rs` | Selective RAM boundaries, flag combinations, I/O reset, display rendering, DMA/timer latches, and diagnostics |
| `crates/gba-core/tests/bios/reset.rs` | All restart flags, exact RAM boundaries, CPU banks, restart execution, device continuity, and IRQ masking |
| `crates/gba-core/tests/bios/arithmetic.rs` | Arithmetic boundaries, wide-integer references, status restoration, and zero-division diagnostics |
| `crates/gba-core/tests/bios/affine.rs` | Matrix/origin references, all angle phases, strides, live rendering, register preservation, and diagnostics |
| `crates/gba-core/tests/bios/angles.rs` | Fixed-point references, axes/quadrants, rounding, caller flags, stack bounds, DMA, and diagnostics |
| `crates/gba-core/tests/bios/lz77.rs` | Token-reference tests, overlaps, output widths, malformed streams, and partial failures |
| `crates/gba-core/tests/bios/run_length.rs` | All block controls, mixed streams, output widths, status, diagnostics, DMA, and IRQ masking |
| `crates/gba-core/tests/bios/bit_unpack.rs` | Width pairs, bit references, offsets, maximum length, memory boundaries, status, DMA, and diagnostics |
| `crates/gba-core/tests/bios/differential.rs` | Round trips, wraparound, output widths, large lengths, partial failures, status, DMA, and diagnostics |
| `crates/gba-core/tests/bios/huffman.rs` | Independent path encoding, tree bounds, packing, large lengths, partial failures, status, and DMA |
