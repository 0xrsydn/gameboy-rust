# References

External sources consulted while implementing the emulator. Add new sources here, or in a "References" section of the matching `docs/hardware/` file when a link explains one specific behavior.

Hardware references used for the core:

- [GBATEK ARM instruction cycle times](https://problemkaputt.de/gbatek-arm-cpu-instruction-cycle-times.htm).
- [GBATEK GBA system control and WAITCNT](https://problemkaputt.de/gbatek-gba-system-control.htm), including HALTCNT power-mode selection.
- [GBATEK BIOS reset functions](https://problemkaputt.de/gbatek-bios-reset-functions.htm), for SoftReset state and restart selection, plus RegisterRamReset flags, RAM bounds, and forced blank.
- [GBATEK BIOS halt functions](https://problemkaputt.de/gbatek-bios-halt-functions.htm), for wait contracts, STOP clock gating, allowed wake sources, and the clock-off IF note.
- [GBATEK BIOS function calling conventions](https://problemkaputt.de/gbatek-bios-functions.htm).
- [GBATEK BIOS memory-copy services](https://problemkaputt.de/gbatek-bios-memory-copy.htm).
- [GBATEK BIOS rotation/scaling services](https://problemkaputt.de/gbatek-bios-rotation-scaling-functions.htm), for record layouts, angle units, and output strides.
- [VisualBoyAdvance-M BIOS services](https://github.com/visualboyadvance-m/visualboyadvance-m/blob/master/src/core/gba/internal/gbaBios.cpp), reviewed for integer affine rounding, signed matrix intermediates, and SoftReset status handling. Our sine table is generated mathematically.
- [GBATEK BIOS arithmetic services](https://problemkaputt.de/gbatek-bios-arithmetic-functions.htm).
- [GBATEK BIOS decompression services](https://problemkaputt.de/gbatek-bios-decompression-functions.htm), including BitUnPack descriptor fields, differential-filter headers, Huffman tree layout, and width constraints.
- [mGBA BIOS service implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/bios.c), reviewed for RegisterRamReset register defaults, angle-polynomial coefficients and quadrant conventions, BitUnPack ordering/offsets, and Huffman tree layout and packing.
- [HALTCNT hardware-access tests](https://github.com/mgba-emu/mgba/issues/2309), for BIOS-only writes and halfword access.
- [GBATEK display status and IRQs](https://problemkaputt.de/gbatek-lcd-i-o-interrupts-and-status.htm).
- [GBATEK display dimensions and timings](https://problemkaputt.de/gbatek-lcd-dimensions-and-timings.htm).
- [mGBA display implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/video.c), used to cross-check hidden-line HBlank IRQs and comparison-write edges.
- [GBATEK affine background registers](https://problemkaputt.de/gbatek-lcd-i-o-bg-rotation-scaling.htm).
- [NanoBoyAdvance background implementation](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/background.cc), reviewed for internal origins, line-end increments, and affine mosaic behavior.
- [GBATEK text and affine map layouts](https://problemkaputt.de/gbatek-lcd-vram-bg-screen-data-format-bg-map.htm).
- [GBATEK background control](https://problemkaputt.de/gbatek-lcd-i-o-bg-control.htm).
- [GBATEK tile/map memory layout](https://problemkaputt.de/gbatek-lcd-vram-overview.htm).
- [mGBA text-background renderer](https://github.com/mgba-emu/mgba/blob/master/src/gba/renderers/software-mode0.c), reviewed for out-of-range fetch behavior.
- [GBATEK sprite overview and rendering limits](https://problemkaputt.de/gbatek-lcd-obj-overview.htm).
- [mGBA sprite preprocessing](https://github.com/mgba-emu/mgba/blob/master/src/gba/renderers/common.c) and [row preparation](https://github.com/mgba-emu/mgba/blob/master/src/gba/renderers/video-software.c), reviewed for inspection costs and aggregate work limits.
- [GBATEK sprite attributes](https://problemkaputt.de/gbatek-lcd-obj-oam-attributes.htm).
- [GBATEK sprite rotation/scaling parameters](https://problemkaputt.de/gbatek-lcd-obj-oam-rotation-scaling-parameters.htm).
- [GBATEK sprite tile mapping](https://problemkaputt.de/gbatek-lcd-obj-vram-character-tile-mapping.htm).
- [Tonc regular sprites](https://gbadev.net/tonc/regobj.html).
- [mGBA sprite renderer](https://github.com/mgba-emu/mgba/blob/master/src/gba/renderers/software-obj.c), reviewed for tile alignment and wrapping.
- [GBATEK bitmap backgrounds](https://problemkaputt.de/gbatek-lcd-vram-bitmap-bg-modes.htm).
- [GBATEK display control](https://problemkaputt.de/gbatek-lcd-i-o-display-control.htm).
- [GBATEK window registers and priority](https://problemkaputt.de/gbatek-lcd-i-o-window-feature.htm).
- [GBATEK color special effects](https://problemkaputt.de/gbatek-lcd-i-o-color-special-effects.htm).
- [GBATEK mosaic register](https://problemkaputt.de/gbatek-lcd-i-o-mosaic-function.htm).
- [NanoBoyAdvance text sampling](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/background.inl), reviewed for vertical counter subtraction.
- [NanoBoyAdvance mosaic registers](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/registers.cc), reviewed for phase-preserving size writes and documented timing uncertainties.
- [Horizontal sprite mosaic hardware findings](https://github.com/mgba-emu/mgba/issues/2933), for latch transitions and transparent metadata.
- [NanoBoyAdvance sprite pipeline](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/sprite.cc), reviewed for one-row-ahead preparation, mosaic phase, priority updates, and OBJ-window exclusions.
- [NanoBoyAdvance display scheduling](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/ppu.cc), reviewed for cycle40 sprite initialization, line227 preparation, and vertical window comparisons on hidden lines.
- [NanoBoyAdvance composition](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/merge.cc), reviewed for palette lookup and horizontal mosaic after sprite preparation.
- [mGBA window renderer](https://github.com/mgba-emu/mgba/blob/master/src/gba/renderers/video-software.c), reviewed for inverted bounds and vertical edge flags.
- [NanoBoyAdvance window implementation](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/ppu/window.cc), reviewed for persistent vertical flags, four-cycle horizontal comparisons, offscreen columns, and end-edge precedence.
- [GBATEK keypad input](https://problemkaputt.de/gbatek-gba-keypad-input.htm), for KEYINPUT/KEYCNT fields and selected-button OR/AND matching.
- [mGBA keypad sampling](https://github.com/mgba-emu/mgba/blob/master/src/gba/gba.c) and [control writes](https://github.com/mgba-emu/mgba/blob/master/src/gba/io.c), reviewed for repeated OR requests, AND snapshot suppression, and newly selected held keys.
- [NanoBoyAdvance keypad implementation](https://github.com/nba-emu/NanoBoyAdvance/blob/master/src/nba/src/hw/keypad/keypad.cc), compared for control access widths; its AND/retrigger behavior differs from the chosen polling model.
- [mGBA keypad timing issue](https://github.com/mgba-emu/mgba/issues/2490), for frontend frame-based input timing limits.
- [Tonc hardware interrupts](https://gbadev.net/tonc/interrupts.html), for keypad source enable and IF/IE bit 12.
- [GBATEK memory mirrors, video byte writes, and unused-memory reads](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm), for unused address ranges, ARM PC+8 open bus, byte lanes, the distinct Thumb region/history rules, and PC-dependent BIOS read protection.
- [nocash GBA open-bus discussion](https://www.ngemu.com/threads/gba-open-bus.170809/), for the distinction between ARM prefetch and region-dependent Thumb values.
- [mGBA memory implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/memory.c), reviewed for `GBALoadBad`, ARM prefetch sourcing, byte/halfword lane selection, and separate DMA/history behavior. Also reviewed its BIOS region-exit prefetch retention and protected-load lane selection.
- [GBATEK GBA memory map and bus widths](https://problemkaputt.de/gbatek-gba-memory-map.htm).
- [GBATEK GBA DMA transfers](https://problemkaputt.de/gbatek-gba-dma-transfers.htm).
- [mGBA DMA implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/dma.c), used to cross-check masks, repeat behavior, and Game Pak source increments.
- [GBATEK GBA timers](https://problemkaputt.de/gbatek-gba-timers.htm).
- [GBATEK GBA interrupt control](https://problemkaputt.de/gbatek-gba-interrupt-control.htm).

- [GBATEK processor registers and modes](https://problemkaputt.de/gbatek-arm-cpu-register-set.htm).
- [GBATEK status transfers](https://problemkaputt.de/gbatek-arm-opcodes-psr-transfer-mrs-msr.htm).
- [GBATEK CPU exceptions](https://problemkaputt.de/gbatek-arm-cpu-exceptions.htm).
- [GBATEK ARM data processing](https://problemkaputt.de/gbatek-arm-opcodes-data-processing-alu.htm), including R15/status notes. Its legacy `{P}` description alone does not establish ARM7 behavior.
- [mGBA ARM instruction implementation](https://github.com/mgba-emu/mgba/blob/master/src/arm/isa-arm.c), reviewed for Rd=15 test/compare SPSR restoration, User/System fallback, and no result write/refill. Also reviewed its word/byte/halfword/signed load ordering: writeback precedes the destination result.
- [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests/tree/a7113b67e63f83a9b321696ddd7042ccfad6c881), pinned for independent ARM testing. Reviewed its MIT license, startup, result macros, compare/status tests, and load/writeback alias tests.
- [Pinned jsmolka Thumb entry](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/thumb.asm), [branch tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/branches.asm), and [memory tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/memory.asm), reviewed for r7 failure IDs, success-marker reset, the Thumb-to-ARM evaluation bridge, and empty-list transfer expectations.
- [Pinned jsmolka memory entry](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/memory.asm), [mirror tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/mirrors.asm), and [video-byte tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/video_strb.asm), reviewed for the r12 checkpoint, binary exit targets, limited byte-write assertions, and mode-selection caveats.
- [gbadoc memory layout](https://gbadev.net/gbadoc/memory.html), consulted for VRAM mirror ranges alongside GBATEK's video-memory layout and byte-write rules.
- [Pinned jsmolka BIOS test](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/bios/bios.asm), reviewed for the r12 evaluation checkpoint, internal VBlank wait, and Nintendo-firmware-specific protected-read expectations after boot, SWI, and IRQ.
- [armwrestler-gba-fixed](https://github.com/destoer/armwrestler-gba-fixed) and [arm7wrestler](https://github.com/Arisotura/arm7wrestler), considered as test sources; not integrated.
- [SingleStepTests ARM7TDMI](https://github.com/SingleStepTests/ARM7TDMI), considered as an experimental, emulator-generated test source; not integrated.
- [GBATEK ARM single data transfers](https://problemkaputt.de/gbatek-arm-opcodes-memory-single-data-transfer-ldr-str-pld.htm), for address calculation, indexing, load extension, and nominal timing.
- [Arm armasm LDR constraints](https://support.arm.com/documentation/dui0801/l/A32-and-T32-Instructions/LDR--register-offset---A32-), reviewed to distinguish portable assembly restrictions from the ARM7 alias behavior tested here.
- [Pinned halfword alias tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/halfword_transfer.asm), for loaded-value precedence in pre/post-indexed `LDRH` aliases.
- [GBATEK memory alignment](https://problemkaputt.de/gbatek-arm-cpu-memory-alignments.htm).
- [GBATEK multiply instructions](https://problemkaputt.de/gbatek-arm-opcodes-multiply-and-multiply-accumulate-mul-mla.htm).
- [GBATEK block transfers](https://www.problemkaputt.de/gbatek-arm-opcodes-memory-block-data-transfer-ldm-stm.htm).
- [GBATEK swaps](https://www.problemkaputt.de/gbatek-arm-opcodes-memory-single-data-swap-swp.htm).
- [GBATEK Thumb register operations](https://problemkaputt.de/gbatek-thumb-opcodes-register-operations-alu-bx.htm).
- [GBATEK Thumb loads/stores](https://problemkaputt.de/gbatek-thumb-opcodes-memory-load-store-ldr-str.htm).
- [GBATEK Thumb address calculation](https://problemkaputt.de/gbatek-thumb-opcodes-memory-addressing-add-pc-sp.htm).
- [GBATEK Thumb stack and multiple transfers](https://problemkaputt.de/gbatek-thumb-opcodes-memory-multiple-load-store-push-pop-and-ldm-stm.htm), for empty-list PC transfers and 64-byte writeback.
- [mGBA Thumb transfer implementation](https://github.com/mgba-emu/mgba/blob/master/src/arm/isa-thumb.c) and [shared memory transfers](https://github.com/mgba-emu/mgba/blob/master/src/gba/memory.c), reviewed for empty STM/PUSH routing and the extra Thumb instruction width on the stored pipeline PC. This supports executing PC+6, rather than an ordinary PC+4 operand read.
- [GBATEK Thumb jumps and calls](https://problemkaputt.de/gbatek-thumb-opcodes-jumps-and-calls.htm).

BIOS protected-read root-cause research:

- [Smolka Progress Report #6](https://www.smolka.dev/posts/progress-report-6), for firmware-dependent protected reads and the Ruby/Sapphire null-pointer case; the report distinguishes Emerald.
- [mGBA: Cracking the GBA BIOS](https://mgba.io/2017/06/30/cracking-gba-bios/), for BIOS access protection and replacement-versus-external firmware approaches.
- [Cult-of-GBA BIOS](https://github.com/Cult-of-GBA/BIOS/tree/a30e9a96df083628b650724b7d4d7112b4070b98), reviewed for original boot, SWI, and IRQ exit layout and its MIT license. No source or binary was copied.
- [mGBA BIOS implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/bios.c), for the explicit post-SWI `biosPrefetch` compatibility value.
- [no$gba BIOS FAQ](https://problemkaputt.de/gbabios.htm), for the distinction between simulated firmware services and using a firmware image.

See [the root-cause analysis and implementation decision](research/bios-readback.md).

Use [GBATEK](https://problemkaputt.de/gbatek.htm) and ARM7TDMI documentation for further hardware work.
Only use game ROMs that you may lawfully use. Do not commit game ROMs, BIOS files, or game assets.
The ignore file excludes `roms/`, common game ROM extensions, and save files.
