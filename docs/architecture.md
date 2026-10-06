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
| `tools/prepare_gba_tests.py`, `tools/gba-tests-{arm,thumb,memory,bios}.lock.json` | Hash-verified public test source/ROM preparation and suite generation |
| `tools/test_prepare_gba_tests.py` | Offline adapter tests using original synthetic data |
| `tools/prepare_homebrew_pong.py`, `tools/homebrew-pong.lock.json` | Hash-pinned homebrew preparation and source-derived gameplay scenarios |
| `tools/test_prepare_homebrew_pong.py` | Offline homebrew preparation, provenance, bounds, and destination-safety tests |
| `.envrc` | Automatic environment loading through direnv |
| `.pi/prompts/jev-gb-debug.md`, `.pi/skills/jev-gb-debug/SKILL.md` | Project-local optional Jev diagnostic workflow |
| `tools/jev_gb_debug.py`, `tools/jev-gb-rubric.json` | Bounded evidence validation, optional classifier request, and local evaluation records |
| `tools/test_jev_gb_debug.py`, `tools/fixtures/jev/` | Offline integration tests and original synthetic classifier probes |
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
| `crates/gba-core/src/cpu/bios_access_tests.rs` | Original CPU BIOS protection regressions for retained history, modes, widths, exception returns, and diagnostics |
| `crates/gba-core/src/cpu/bios_write_tests.rs` | Ignored CPU/DMA BIOS writes, immutable images, swaps, timing, completion, and strict unsupported boundaries |
| `crates/gba-core/src/cpu/thumb_bios_access_tests.rs` | Thumb BIOS aligned-word snapshots, transitions, load lanes, boundaries, diagnostics, and nominal timing |
| `crates/gba-core/src/cpu/thumb_open_bus_tests.rs` | Region-dependent Thumb open bus, mirrors, widths, transfers, source boundaries, isolation, and nominal timing |
| `crates/gba-core/src/cpu/iwram_open_bus_tests.rs` | Persistent IWRAM lanes, cold fills, access widths, isolation, errors, refills, IRQ preservation, and nominal timing |
| `crates/gba-core/src/cpu/iwram_dma_tests.rs` | DMA lane updates, cold/resumed fetch ordering, channel isolation, diagnostics, and IRQ preservation |
| `crates/gba-core/src/cpu/iwram_refill_tests.rs` | Thumb IWRAM target-pair refills, state changes, exception returns, boundaries, isolation, and nominal costs |
| `crates/gba-core/src/memory/iwram_bus.rs` | Transactional persistent IWRAM lanes for ARM/Thumb fetches, CPU data accesses, and DMA |
| `crates/gba-core/src/memory/iwram_history_tests.rs` | Cross-state latch ownership, ARM accesses/refills, nonlocal code, pre-start DMA, IRQ preservation, and rollback |
| `crates/gba-core/src/cpu/compare_psr_tests.rs` | Original Rd=15 test/compare status restoration, flags, bank, and timing regressions |
| `crates/gba-core/src/cpu/status_tests.rs`, `crates/gba-core/src/cpu/exception_tests.rs` | Status, banking, exception, and return tests |
| `crates/gba-core/src/cpu/thumb_tests.rs` | Thumb instructions, state switching, and decoder checks |
| `crates/gba-core/src/cpu/thumb_empty_tests.rs` | Original empty-list stored-PC regressions for banks, alignment, writeback, timing, and diagnostics |
| `crates/gba-core/src/cpu/transfer_tests.rs` | Stack, addressing, overlap, empty-list, and swap tests |
| `crates/gba-core/src/cpu/tests.rs` | Condition truth tables and immediate arithmetic tests |
| `crates/gba-core/src/cpu/instruction_tests.rs` | Register operations, transfers, branches, and edge cases |
| `crates/gba-core/src/cartridge.rs` | Explicit GPIO/RTC selection, ROM read overlay, pin directions, caller-clock routing, and bounded diagnostics |
| `crates/gba-core/src/cartridge/flash.rs` | Explicit save devices, Macronix identification/read controller, bank selection, and unsupported-write diagnostics |
| `crates/gba-core/tests/flash.rs` | Erased/supplied images, chip IDs, all bank addresses, invalid commands, reset limits, DMA rejection, and RTC independence |
| `crates/gba-core/src/memory/flash_step_tests.rs` | ARM/Thumb Flash writes and RAM reads, wait costs, staged IDs, fetch restrictions, and failed-step/preflight isolation |
| `crates/gba-core/src/cartridge/rtc.rs` | RTC framing, control/calendar commands, read snapshots, complete writes, and reset |
| `crates/gba-core/src/cartridge/calendar.rs` | Validated decimal calendar, BCD representation, checked epoch conversion, and bounded elapsed-time arithmetic |
| `crates/gba-core/src/cartridge/calendar/tests.rs` | Calendar field validation, century-wide rollovers, hour modes, and large/split advances |
| `crates/gba-core/tests/cartridge_gpio/calendar.rs` | Calendar wire transfers, snapshots, aborts, invalid payloads, reset, and independent HALT/STOP time |
| `crates/gba-core/tests/cartridge_gpio.rs` | GPIO lanes, ROM preservation, RTC framing/control/aborts, unsupported commands, and DMA3 access |
| `crates/gba-core/src/memory/cartridge_step_tests.rs` | ARM/Thumb GPIO timing, rejected command-edge rollback, block preflight, and staged fetch/read overlays |
| `crates/gba-core/src/memory.rs` | Memory mapping, I/O routing, write validation, and device clock |
| `crates/gba-core/src/memory/fetch.rs` | Strict instruction samples with captured region-dependent bus observations |
| `crates/gba-core/src/memory/fetch_tests.rs` | Fetch widths, bus lanes, captured refill/BIOS values, strict errors, and side-effect isolation |
| `crates/gba-core/src/cpu/fetch_history_tests.rs` | Shared sample consumption, no execution-entry rereads, and failed-step retry |
| `crates/gba-core/src/cpu/fetch_boundary_tests.rs` | Actual fetch-region snapshots, split refills, IWRAM lane requirements, ROM-window boundaries, and nominal timing |
| `crates/gba-core/src/io.rs` | I/O registers, timers, HALT/STOP wake-up, next-event bounds, and IRQ latches |
| `crates/gba-core/src/audio.rs` | Direct Sound state, PSG sequencer, idle channel/wave banks, and instantaneous stereo mixing |
| `crates/gba-core/src/audio/pulse.rs` | Shared pulse oscillator; sweep is mapped only for channel 1 |
| `crates/gba-core/src/audio/modulation.rs` | Shared PSG length, envelope, logical DAC gate, and activity state |
| `crates/gba-core/src/audio/noise.rs` | Noise channel 4 divider, 7/15-bit counter, and bounded jump-ahead advancement |
| `crates/gba-core/src/audio/noise/tests.rs` | Counter periods, independent bit-array reference, divider fields, and clock batching |
| `crates/gba-core/tests/noise_sound.rs` | Noise registers, modulation, stereo mixing, independent status, HALT/STOP, and capture independence |
| `crates/gba-core/src/audio/pulse/tests.rs` | Exhaustive frequency/duty arithmetic and original modulation edge tests |
| `crates/gba-core/tests/pulse_sound.rs` | Register masks, stereo mixing, sequencer clocks, idle modes, batching, and explicit limits |
| `crates/gba-core/tests/pulse_sound/channel2.rs` | Channel 2 mapping, register gaps, independent state, shared clocks, and sum-before-rounding mixing |
| `crates/gba-core/tests/direct_sound.rs` | FIFO lanes/reset, signed mixing, timer selection, DMA refill, and HALT/STOP |
| `crates/gba-core/src/memory/audio_step_tests.rs` | Audio access phases, shadow validation, and CPU/DMA failure rollback |
| `crates/gba-core/src/io/serial.rs` | Disconnected normal serial shifter, internal clocks, external waiting, idle multiplayer and local Joybus registers, GPIO latches, and live-write validation |
| `crates/gba-core/tests/inactive_devices.rs` | Device masks, wave RAM, GPIO pull-ups, serial data, and failed CPU/DMA write isolation |
| `crates/gba-core/tests/serial_control.rs` | RCNT mode gating, inactive bits, byte/halfword lanes, CPU/DMA diagnostics, and interrupt isolation |
| `crates/gba-core/tests/serial_external.rs` | External-clock waiting, cancellation, HALT/STOP, ARM/Thumb/DMA starts, and atomic diagnostics |
| `crates/gba-core/tests/serial_internal.rs` | Nominal bit edges, widths/rates, cancellation, completion IRQs, HALT/STOP, and live-write limits |
| `crates/gba-core/tests/serial_joybus.rs` | Local Joybus masks/data, pending status, inactive IRQs, mode gating, HALT/STOP, DMA, and padding diagnostics |
| `crates/gba-core/src/memory/joybus_step_tests.rs` | Joybus ARM/Thumb stores, staged reads, normal completion boundaries, and failed block-store isolation |
| `crates/gba-core/tests/serial_multiplayer.rs` | Disconnected child status, receive/send lanes, mode aliases, ignored child start, HALT/STOP, and ARM/Thumb/DMA access |
| `crates/gba-core/src/memory/multiplayer_step_tests.rs` | Multiplayer mode changes at normal completion, block-transfer preflight, staged reads, and atomic diagnostics |
| `crates/gba-core/src/memory/serial_step_tests.rs` | Serial bus-phase observations, preflight validation, DMA, IF ordering, capture isolation, and rollback |
| `crates/gba-core/src/io/timer_step.rs` | Staged timer/audio/serial state, divider phase, timer/serial IF bits, and sound refill requests at ordered CPU/DMA bus phases |
| `crates/gba-core/src/memory/timer_step_tests.rs` | Timer access phases, coherent lanes, cascades, IF ordering, rollback, DMA, idle, and clock ownership |
| `crates/gba-core/src/display.rs` | Display clock, scanline status, comparison edges, and display IRQ events |
| `crates/gba-core/src/dma.rs` | DMA registers, internal pointers, retained channel data, trigger state, priority, and completion IRQs |
| `crates/gba-core/src/machine.rs` | Timed CPU/DMA/device stepping and IRQ delivery |
| `crates/gba-core/src/timing.rs` | Bus widths, wait-state costs, ordered CPU timing transactions, and timing breakdowns |
| `crates/gba-core/src/timing/prefetch.rs` | Nominal eight-halfword opcode queue, partial transfers, capacity stops, and cancellation |
| `crates/gba-core/src/timing/prefetch/tests.rs` | Wait-state matrices, partial words, cancellation phases, boundaries, configuration, and progress consistency |
| `crates/gba-core/src/memory/prefetch_tests.rs` | CPU/DMA/idle queue ownership, WAITCNT writes, branch/IRQ timing, API isolation, and rollback |
| `crates/gba-core/src/cpu/pipeline.rs` | CPU-owned ARM/Thumb instructions, PC/state tags, next fetch kind, deferred diagnostics, refills, and invalidation |
| `crates/gba-core/src/cpu/arm_pipeline_tests.rs` | ARM self-modifying code, branches, target pairs, DMA isolation, exceptions, errors, and timing equivalence |
| `crates/gba-core/src/cpu/thumb_pipeline_tests.rs` | Thumb retained halfwords, BL prefix/suffix, state-tagged refills, DMA isolation, rollback, and timing equivalence |
| `crates/gba-core/src/cpu/timing.rs` | CPU timing transaction entry/exit, IRQ integration, and incoming instruction classification |
| `crates/gba-core/src/cpu/fetch_timing_tests.rs` | Fetch-region/page/wait-window costs, DMA resume, WAITCNT stores, deferred errors, and timer/IRQ effects |
| `crates/gba-core/src/cpu/refill_timing_tests.rs` | Refill source/target widths, wait settings, boundaries, DMA resume, and IRQ diagnostics |
| `crates/gba-core/src/cpu/fetch_sequence_tests.rs` | Instruction-pair access kinds, transfer/internal-cycle breaks, refills, IRQ/DMA composition, and rollback |
| `crates/gba-core/src/memory/irq_fetch_tests.rs` | Discarded IRQ fetch lanes, cold/retained entry, BIOS-history isolation, and deferred vector errors |
| `crates/gba-core/src/memory/timing_event_tests.rs` | Ordered source/data/internal/refill events, WAITCNT snapshots, IRQ entry, rollback, and host/DMA isolation |
| `crates/gba-core/src/cpu/dma_resume_tests.rs` | One-shot non-sequential DMA resume costs, wait settings, failures, idle, IRQs, and API isolation |
| `crates/gba-core/src/cpu/timing_tests.rs` | Instruction timing and semantic-equivalence checks |
| `crates/gba-demos/src/timer_demo.rs` | Original timer-configuration program and IRQ handler |
| `crates/gba-demos/src/prefetch_probe.rs` | Original bounded ARM read/control probes and pinned published cancellation observations |
| `crates/gba-demos/examples/prefetch_cancellation.rs` | CSV comparison of published, instruction-boundary, and unadjusted timer intervals; fails on mismatches |
| `crates/gba-core/tests/prefetch_cancellation.rs` | Published read comparisons, cancellation phase, disabled-prefetch controls, and unadjusted timer samples |
| `Cargo.toml` | Workspace members and shared package settings |
| `crates/gba-core/src/lib.rs` | Core modules |
| `crates/gba-demos/src/lib.rs` | Demo modules, original instruction/exception bytes, and raw input-test ROM |
| `crates/gba-core/src/bios.rs` | Original ARM BIOS image builder, minimal boot, IRQ dispatch, waits, and memory services |
| `crates/gba-core/tests/bios/boot_video.rs` | Firmware identity scales, bitmap/affine startup rendering, capture, and raw-memory isolation |
| `crates/gba-core/tests/bios/copy_bus.rs` | Original copy/fill services with ignored BIOS destinations and retained source diagnostics |
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
| `crates/desktop/src/cartridge/suite/gameplay_tests.rs` | VBlank completion, input snapshots, captured pixels, and rendering failures |
| `crates/desktop/tests/rom_suite_cli.rs` | File-backed suite execution, case isolation, output schema, determinism, and exit status |
| `crates/desktop/examples/write_test_suite.rs` | Original ARM/Thumb/BIOS fixture generator with a local JSON suite |
| `crates/desktop/src/cartridge/rtc_clock.rs` | Host-only UTC seed and monotonic elapsed-time adapter, with injected epoch/duration tests |
| `crates/desktop/src/cartridge/window.rs` | ROM-window session, bounded slices, capture, STOP/input handling, and progress limits |
| `crates/desktop/src/cartridge/window/tests.rs` | Original ROM pixels, keyboard mapping, STOP wake, HALT, frame budgets, and diagnostics |
| `crates/desktop/src/desktop/rom.rs` | Main-thread ROM window, input polling, presentation pacing, and frame-limited exit |
| `crates/desktop/examples/write_rom_demo.rs` | Non-overwriting export of the original input-test ROM |
| `crates/desktop/src/cartridge/tests.rs` | ROM options, bounded readers, instruction/DMA/IRQ budgets, HALT/STOP, and diagnostic reports |
| `crates/desktop/tests/rom_cli.rs` | File loading and exit-status checks; opt-in native ROM-window integration test |
| `crates/gba-core/tests/core.rs` | CPU integration tests and the complete demo |
| `crates/gba-core/tests/memory.rs` | Memory widths, alignment, errors, and mirrors |
| `crates/gba-core/tests/video_bus_cpu.rs` | Original CPU video-byte writes, exact sentinels, mirror/mode boundaries, readback, and nominal store timing |
| `crates/gba-core/tests/exceptions.rs` | Exception demo, BIOS mapping, and reset-vector execution |
| `crates/gba-core/tests/timers.rs` | Timer rules, interrupt registers, and a cycle-by-cycle reference |
| `crates/gba-core/tests/machine.rs` | Device IRQ entry/return, clock policy, and I/O failure atomicity |
| `crates/gba-core/tests/timing.rs` | WAITCNT fields, bus costs, and timer/IRQ timing integration |
| `crates/gba-core/tests/timer_prescaler.rs` | Exhaustive divider phases, staggered starts, control/cascade transitions, independent edge simulation, large batches, and idle ownership |
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
| `crates/gba-core/tests/dma/latch.rs` | Retained channel data, blocked sources, lane selection, isolation, errors, repeats, and BIOS protection |
| `crates/gba-core/tests/stop.rs` | STOP clock gating, live keypad wake, retained DMA/device phases, frame-runner results, and access rules |
| `crates/gba-core/tests/bios/stop.rs` | ARM/Thumb Stop calls, caller preservation, keypad wake, and stopped frame running |
| `crates/gba-core/tests/halt.rs` | HALT wake masks, idle timing, BIOS-only power writes, DMA progress, and validation |
| `crates/gba-core/tests/bios.rs` | ARM/Thumb service calls, copy/fill boundaries, wait races, callback contracts, and boot |
| `crates/gba-core/tests/bios/readback.rs` | Image-derived boot/reset/SWI/IRQ compatibility words, load lanes, caller state, and supplied-image isolation |
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
