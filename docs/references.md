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
- [GBATEK memory mirrors and video byte writes](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm).
- [GBATEK GBA memory map and bus widths](https://problemkaputt.de/gbatek-gba-memory-map.htm).
- [GBATEK GBA DMA transfers](https://problemkaputt.de/gbatek-gba-dma-transfers.htm).
- [mGBA DMA implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/dma.c), used to cross-check masks, repeat behavior, and Game Pak source increments.
- [GBATEK GBA timers](https://problemkaputt.de/gbatek-gba-timers.htm).
- [GBATEK GBA interrupt control](https://problemkaputt.de/gbatek-gba-interrupt-control.htm).

- [GBATEK processor registers and modes](https://problemkaputt.de/gbatek-arm-cpu-register-set.htm).
- [GBATEK status transfers](https://problemkaputt.de/gbatek-arm-opcodes-psr-transfer-mrs-msr.htm).
- [GBATEK CPU exceptions](https://problemkaputt.de/gbatek-arm-cpu-exceptions.htm).
- [GBATEK ARM data processing](https://problemkaputt.de/gbatek-arm-opcodes-data-processing-alu.htm), including R15/status notes. Its legacy `{P}` description alone does not establish ARM7 behavior.
- [mGBA ARM instruction implementation](https://github.com/mgba-emu/mgba/blob/master/src/arm/isa-arm.c), reviewed for Rd=15 test/compare SPSR restoration, User/System fallback, and no result write/refill.
- [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests/tree/a7113b67e63f83a9b321696ddd7042ccfad6c881), pinned for independent ARM testing. Reviewed its MIT license, startup, result macros, compare/status tests, and load/writeback alias tests.
- [armwrestler-gba-fixed](https://github.com/destoer/armwrestler-gba-fixed) and [arm7wrestler](https://github.com/Arisotura/arm7wrestler), considered as test sources; not integrated.
- [SingleStepTests ARM7TDMI](https://github.com/SingleStepTests/ARM7TDMI), considered as an experimental, emulator-generated test source; not integrated.
- [GBATEK ARM single data transfers](https://problemkaputt.de/gbatek-arm-opcodes-memory-single-data-transfer-ldr-str-pld.htm).
- [GBATEK memory alignment](https://problemkaputt.de/gbatek-arm-cpu-memory-alignments.htm).
- [GBATEK multiply instructions](https://problemkaputt.de/gbatek-arm-opcodes-multiply-and-multiply-accumulate-mul-mla.htm).
- [GBATEK block transfers](https://www.problemkaputt.de/gbatek-arm-opcodes-memory-block-data-transfer-ldm-stm.htm).
- [GBATEK swaps](https://www.problemkaputt.de/gbatek-arm-opcodes-memory-single-data-swap-swp.htm).
- [GBATEK Thumb register operations](https://problemkaputt.de/gbatek-thumb-opcodes-register-operations-alu-bx.htm).
- [GBATEK Thumb loads/stores](https://problemkaputt.de/gbatek-thumb-opcodes-memory-load-store-ldr-str.htm).
- [GBATEK Thumb address calculation](https://problemkaputt.de/gbatek-thumb-opcodes-memory-addressing-add-pc-sp.htm).
- [GBATEK Thumb stack and multiple transfers](https://problemkaputt.de/gbatek-thumb-opcodes-memory-multiple-load-store-push-pop-and-ldm-stm.htm).
- [GBATEK Thumb jumps and calls](https://problemkaputt.de/gbatek-thumb-opcodes-jumps-and-calls.htm).

Use [GBATEK](https://problemkaputt.de/gbatek.htm) and ARM7TDMI documentation for further hardware work.
Only use game ROMs that you may lawfully use. Do not commit game ROMs, BIOS files, or game assets.
The ignore file excludes `roms/`, common game ROM extensions, and save files.
