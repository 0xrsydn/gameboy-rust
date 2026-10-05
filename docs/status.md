# Project status

What the emulator does today, what it deliberately leaves out, and what comes next.
Update this file when a feature lands or a limit is removed.

## Current behavior

- Native desktop window, with animation and keyboard controls for display testing.
- Platform-independent 240×160 framebuffer and GBA RGB555 color conversion.
- Optional row capture at HBlank, double-buffered frame publication at VBlank, and retained mid-frame changes.
- CPU-configured HBlank DMA raster demo with per-row palette changes.
- Mode 0 tile backgrounds: 4-bit/8-bit pixels, palettes, flips, scrolling, map sizes, transparency, and priorities.
- Mode 3 video RAM snapshots, BG2 enable, palette backdrop, forced blank, and green swap.
- Mode 1 mixed text/affine backgrounds and Mode 2 affine backgrounds, with signed transforms and map wrapping/clipping.
- Mode 3/4/5 bitmap transforms through BG2 affine registers, with Mode 4/5 page selection.
- Mode 4 indexed colors and transparent index zero; Mode 5 opaque 160×128 RGB555 pixels.
- Regular and affine sprites in Modes 0–5: palettes, rotation, scaling, flips, clipping, and priority selection.
- One-line-ahead sprite row preparation, with buffered palette indices and metadata.
- Background and sprite mosaic, with independent 1–16 pixel dimensions and per-layer enable flags.
- Separate live vertical mosaic counters for backgrounds and sprites, with phase-preserving size writes.
- WIN0/WIN1 rectangles and OBJ window masks, with per-region layer and color-effect controls.
- Vertical WIN0/WIN1 edge tracking across visible rows, VBlank, and frame boundaries.
- Horizontal WIN0/WIN1 comparator history, including mid-line writes and offscreen edges.
- Alpha blending, brightness increase/decrease, and semi-transparent sprites using five-bit color arithmetic.
- 96 KiB video RAM, 1 KiB palette RAM, and 1 KiB object attribute memory, with mirrors and byte-write behavior.
- Active-low `KEYINPUT` for all ten GBA buttons, independent of the window library.
- `KEYCNT` selection and OR/AND matching, keypad IRQ latching, and HALT wake-up through IE bit 12.
- Original ARM graphics demo that uses DMA, BIOS VBlank waiting, and a ROM-side IRQ callback.
- Sixteen active 32-bit CPU registers, with ARM7 mode-specific register banks.
- User, System, Supervisor, IRQ, FIQ, Abort, and Undefined modes.
- Current and saved program status registers (`CPSR`/`SPSR`), with `MRS`/`MSR` transfers.
- ARM/Thumb software interrupts (`SWI`), exception vectors, and status-restoring returns.
- CPU interrupt entry, interrupt masks, and FIQ priority.
- GBA interrupt enable (`IE`), request flags (`IF`), and master enable (`IME`) registers.
- Four 16-bit timers, with reloads, prescalers, count-up cascading, and timer-generated IRQs.
- A 228-line display clock, VCOUNT/DISPSTAT, and VBlank/HBlank/VCount-match interrupts.
- Four direct memory access (DMA) channels, with immediate, VBlank, and visible-line HBlank triggers.
- Halfword/word DMA, address controls, repeat, channel priority, CPU pausing, and completion IRQs.
- HALT with enabled-request wake-up, continued device clocks, and bounded idle steps.
- BIOS-only CPU writes to POSTFLG/HALTCNT, with HALT and keypad-wake STOP.
- STOP freezes CPU/DMA/device clocks, retains device phases, and reports a stopped frame runner without spinning.
- Machine execution to the next VBlank event, with a configurable step limit.
- CPU/device stepping with nominal ARM7 instruction costs and memory wait states.
- Game Pak wait-state control (`WAITCNT`) for all three ROM windows.
- Sequential/non-sequential data accesses, branch refill costs, and variable multiply timing.
- Optional caller-supplied 16 KiB BIOS mapping for vector code, without CPU BIOS read protection.
- Optional original BIOS replacement: minimal boot, SoftReset, selective RegisterRamReset, IRQ dispatch, interrupt waits, memory copy/fill, integer/fixed-point arithmetic, affine matrices, bit unpacking, LZ77/run-length/Huffman decompression, and differential filters.
- All 16 ARM data-processing operations, with immediate and shifted-register operands.
- Logical operations: `AND`, `EOR`, `TST`, `TEQ`, `ORR`, `MOV`, `BIC`, and `MVN`.
- Arithmetic operations: `SUB`, `RSB`, `ADD`, `ADC`, `SBC`, `RSC`, `CMP`, and `CMN`.
- Logical and arithmetic shifts, rotate-right, and rotate-right through carry (`RRX`).
- Immediate and register-specified shift amounts, including zero and amounts of 32 or more.
- `MUL`, `MLA`, `UMULL`, `UMLAL`, `SMULL`, and `SMLAL`.
- Negative (N), zero (Z), carry (C), and signed overflow (V) flags.
- All 15 defined ARM condition codes, including always (`AL`).
- `B` and `BL` with signed relative displacement and conditional execution.
- `BX` switches between ARM and Thumb using target bit zero.
- `LDR`, `STR`, `LDRB`, `STRB`, `LDRH`, `STRH`, `LDRSB`, and `LDRSH`.
- Single-load base/destination aliases retain the loaded value over writeback for r0–r14, across word, byte, halfword, and signed loads.
- Pre/post-indexed addressing, positive/negative offsets, and base-register writeback.
- `LDM`/`STM` in increment-after, increment-before, decrement-after, and decrement-before modes.
- Stack save/restore, PC loads, empty register lists, and ARM7 base-register overlap rules.
- `SWP`/`SWPB`, including unaligned word rotation and register aliases.
- 256 KiB external work RAM and 32 KiB internal work RAM, including mirrors.
- Read-only cartridge bytes in the three GBA cartridge windows.
- Raw ROM-file loading with original BIOS boot, terminal diagnostics, and final CPU/step reports.
- ROM windows with scanline capture, keyboard input, focus-loss release, bounded execution slices, and interactive keypad wake from STOP.
- Optional ROM-window frame limits and an original input-test ROM generator.
- Headless JSON ROM suites with per-case checkpoints, register/CPSR/memory assertions, bounded budgets, and structured failure reports.
- Hash-pinned preparation of public `jsmolka/gba-tests` ARM and Thumb ROMs, with verified result-register checkpoints.
- The pinned public ARM and Thumb ROMs pass their checkpoints on Darwin arm64 in debug and release builds.
- ARM test/compare Rd=15 status restoration, without a PC result write or nominal refill.
- ARM unused-memory data reads return a PC+8 word snapshot, with normal byte lanes and load rotation.
- Little-endian byte, halfword, and word reads/writes.
- ARM7TDMI word-load rotation, aligned stores, and odd-address halfword-load behavior.
- Explicit errors for unsupported instructions when their condition passes, and for unsupported memory accesses.

Thumb execution includes:

- Immediate and register arithmetic, shifts, comparisons, logical operations, and multiply.
- High-register operations and `BX`.
- Word, byte, halfword, and signed loads/stores with register or immediate offsets.
- PC-relative literal loads, PC/SP-relative address calculation, and stack-pointer adjustment.
- `PUSH`/`POP` and multiple-register loads/stores, including PC+6 stores for empty register lists.
- All 14 conditional branches, unconditional branches, and both halves of `BL`.

## Deliberate limits

This is an interpreter scaffold, not an accurate GBA implementation.
It models CPU reset state and an optional minimal firmware subset, not the complete GBA boot sequence or BIOS service set.
Status handling covers ARM7 flags, modes, interrupt masks, and instruction state; reserved bits read as zero.
The reserved ARM condition code `0xF` and Thumb condition code `0xE` return errors.
ARMv5+ extensions such as `BLX`, `BKPT`, and Thumb-2 instructions are not supported.
The ARMv4T high-register format requires at least one high register; low-to-low forms in that format return errors.
`LDRT`/`STRT` single-transfer variants and invalid register combinations return errors.
BIOS reads are not protected by the executing PC, and BIOS open-bus behavior is not modeled.
Multiply flags N/Z are implemented. Unspecified multiply C/V outputs are preserved deterministically, not hardware-verified.
Instruction and memory errors are development diagnostics, not emulated CPU exceptions.
Block transfers validate all accesses before committing register, RAM, or I/O changes.
This diagnostic policy does not reproduce partial transfers during hardware data aborts.
CPU instructions, including swaps, finish before a newly requested DMA unit runs. Sub-instruction bus arbitration is not modeled.
CPU/bus costs are nominal estimates, not a cycle-accurate implementation.
Graphics support includes Mode 0–5 debug snapshots and HBlank row capture with sprites, mosaic, windows, and color effects.
Internal affine origins, vertical window flags, and vertical mosaic counters track line boundaries.
Horizontal window comparators retain four-cycle event history.
Sprite rows prepare one line ahead with a whole-row cycle40 sample and nominal per-row work limits.
Individual fetch timing, hardware-accurate work cutoffs, video-bus contention, and per-pixel color composition remain unimplemented.
There is no sound or cartridge save support.
Mapped I/O covers DMA, timers, interrupts, WAITCNT, display control/status, background control/scroll/affine registers, window/effect registers, KEYINPUT/KEYCNT, POSTFLG, and HALTCNT.
Keypad IRQs use a documented polling model; hardware retrigger edges and asynchronous input timing remain unverified.
STOP implements keypad wake only. Serial/Game Pak wake sources, oscillator restart delays, and exact wake timing remain unimplemented or unverified.
The presentation framebuffer is separate from emulated video RAM.
All named graphics demos connect desktop input and rendering to the emulated CPU. The default display test remains host-generated.
ROM files can run in bounded terminal mode or a window with the original BIOS replacement.
ROM windows have no demo-specific startup checks. Input is sampled between bounded execution slices.
Audio, saves, and external BIOS loading remain unavailable. ROM loading does not establish commercial-game compatibility.
See [cartridge loading](hardware/cartridge.md) for file limits, step budgets, and exit status.
The [ROM suite runner](rom-tests.md) has original regressions and a [pinned public ARM baseline](public-arm-tests.md).
The pinned public ARM ROM passes its result checkpoint in debug and release builds.
The [pinned public Thumb ROM](public-thumb-tests.md) also passes after correcting its empty-list store PC value.
Other public suites remain unverified. These results do not establish full CPU compatibility.
The runner requires known completion addresses and has no debug-port protocol, scripted input, or rendered-image assertions.

ARM data reads from unused address ranges use a bounded [PC+8 open-bus snapshot](hardware/cpu.md#arm-unused-memory-data-reads).
This is not a fetch pipeline. Thumb, DMA, BIOS-protected reads, and unused/write-only I/O open bus remain unmodeled.
Host inspection and instruction fetches remain strict. Other unmapped reads return errors.
Reads beyond the supplied cartridge bytes also return errors.
Direct memory-bus halfword and word accesses require alignment.
CPU load/store instructions apply ARM7TDMI alignment and rotation rules before accessing the bus.
Writes to cartridge addresses return errors instead of modeling cartridge hardware.

## Next steps

1. Add a pinned public memory suite with a verified result protocol, then address its first compatibility failure.
   Extend open-bus support with verified Thumb region/alignment rules and bus history, rather than a blanket unmapped-read fallback.
2. Refine nominal timing with a fetch pipeline, Game Pak prefetch, per-access device updates, and verified timer/IRQ delays.
3. Validate keypad retrigger behavior with hardware tests and add remaining device registers; extend BIOS reset coverage as sound/serial support becomes available.
   Validate STOP entry/wake edges and add external wake sources and remaining DMA device modes.
4. Replace nominal sprite work limits with verified individual fetch timing; add background fetch timing and per-pixel composition.
5. Validate more original/public test ROMs through the window; add audio, cartridge hardware, and saves before testing Emerald compatibility.

Keep the emulator core independent of window and audio libraries.
Treat Nintendo DS support as a separate project phase.
