# gba-rust

A small Game Boy Advance (GBA) emulator foundation written in Rust.
The core has no external Rust dependencies when built with `--no-default-features`.
The default `desktop` feature adds minifb for a native window.

**This cannot run Pokémon Emerald or other games yet.**
The CPU-driven demos run original ARM code through emulated video RAM.
The default window remains a separate host-generated display test.

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
# Uses nominal instruction/bus costs; prefetch and sub-instruction timing are not modeled.
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

## Development environment

**macOS (Darwin) is the first development and validation target.**
Build and test new features on Apple Silicon macOS before expanding platform support.
Future window, input, and audio libraries must support native macOS.

Install Nix with `nix-command` and `flakes` enabled, plus direnv.
The flake defines shells for Apple Silicon macOS, Intel macOS, ARM64 Linux, and x86-64 Linux.
Only Apple Silicon macOS is verified. Intel macOS and Linux remain untested.

Verified locally on macOS 15.7.3:

- Nix system: `aarch64-darwin`.
- Rust host: `aarch64-apple-darwin`.
- Release executable: native Mach-O `arm64`, without Rosetta.
- All 720 tests pass in debug and release builds.
- All ten native window smoke tests pass. The mosaic test submits 128 frames; the other nine submit 60 frames each.
- The eight CPU-driven smoke tests check CPU state, every output pixel, and presentation at VBlank entry.
- The terminal-only CPU and timer IRQ demos run successfully.

These results cover the CPU core, memory, Mode 0–5 snapshots and row capture, input mapping, and desktop windows.
They do not verify a complete GBA display controller or audio.
Arrow-key movement in the original host display test was confirmed manually on this Mac.
The new CPU graphics demo has automated input-mapping tests; its physical keyboard behavior still needs manual confirmation.

Enable the direnv hook in your shell if it is not already configured.
For zsh, add this line to `~/.zshrc`:

```sh
eval "$(direnv hook zsh)"
```

For bash, use `eval "$(direnv hook bash)"` in `~/.bashrc` instead.
Restart your shell after changing its configuration.

From this project directory:

```sh
direnv allow
cargo run
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

To run commands without a shell hook:

```sh
direnv allow .
direnv exec . cargo run --locked
direnv exec . cargo test --locked
```

`.envrc` uses the Nix flake to load Rust, Cargo, rustfmt, and Clippy.
`flake.lock` pins the Nix package source. Keep it in version control.
The first run needs network access to download the development tools and Rust dependencies.

The pinned Nix Darwin toolchain targets macOS 14.0 or newer.
The flake sets Darwin-only `CFLAGS` to correct two minifb 0.28 native-build issues:

- Override its macOS 10.10 deployment flag, which predates the Metal APIs it uses.
- Use `-fcommon` for its shared tentative Objective-C global definition.

Build through direnv or `nix develop` so these settings apply.
The window runs on the main thread, as required by macOS AppKit.
`src/desktop.rs` also corrects the inverted macOS focus result in minifb 0.28.0.
The dependency is pinned to that exact version. Review this workaround before upgrading minifb.
Intel macOS and Linux desktop builds are not verified.

Without direnv, use:

```sh
nix develop
```

For one command without entering a shell:

```sh
nix develop -c cargo test
```

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
- BIOS-only CPU writes to POSTFLG/HALTCNT, with explicit errors for unsupported STOP mode.
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
- Pre/post-indexed addressing, positive/negative offsets, and base-register writeback.
- `LDM`/`STM` in increment-after, increment-before, decrement-after, and decrement-before modes.
- Stack save/restore, PC loads, empty register lists, and ARM7 base-register overlap rules.
- `SWP`/`SWPB`, including unaligned word rotation and register aliases.
- 256 KiB external work RAM and 32 KiB internal work RAM, including mirrors.
- Read-only cartridge bytes in the three GBA cartridge windows.
- Little-endian byte, halfword, and word reads/writes.
- ARM7TDMI word-load rotation, aligned stores, and odd-address halfword-load behavior.
- Explicit errors for unsupported instructions when their condition passes, and for unsupported memory accesses.

Thumb execution includes:

- Immediate and register arithmetic, shifts, comparisons, logical operations, and multiply.
- High-register operations and `BX`.
- Word, byte, halfword, and signed loads/stores with register or immediate offsets.
- PC-relative literal loads, PC/SP-relative address calculation, and stack-pointer adjustment.
- `PUSH`/`POP` and multiple-register loads/stores.
- All 14 conditional branches, unconditional branches, and both halves of `BL`.

The `--cpu-demo` command first runs the original instruction demo for 40 steps, starting at `0x08000000` in ARM state.
It counts r0 down from three while incrementing r1.
`SUBS` updates the flags. `BNE` repeats the loop until the zero flag is set.
`CMP` checks the result, and `MOVEQ` sets r2 to 42.
The program then stores 42 in work RAM at `0x02000000` and loads it into r4.
The program sets the stack pointer to `0x03000100` in internal work RAM.
A subroutine saves r4 and the link register using `STMDB sp!`.
It shifts r4 left into r5 and multiplies r4 by r5 into r6.
`SWP` writes 3528 to work RAM and returns the old value, 42, in r7.
The subroutine clears r4, then uses `LDMIA sp!` to restore r4 and return through the saved PC.
The program then uses `BX` to enter Thumb code at `0x08000064`.
A Thumb subroutine saves its return address, multiplies six by seven, and returns using `POP {pc}`.
The Thumb code stores 42 at `0x02000004`, checks it, and uses `BX` to return to ARM.
It finishes with `r0=42`, `r1=7`, `r2=42`, `r4=42`, `r5=84`, `r6=3528`, and `r7=42`.
The stack pointer returns to `0x03000100`.
The trace labels each step as `Arm` or `Thumb` and shows flags in `NZCV` order.
The final flags are `0110`.
Its last instruction branches to itself. The host stops after the fixed step count.

The command then runs an 11-step exception demo with a separate CPU and memory.
ARM and Thumb code each execute `SWI` and enter the ARM Supervisor handler through vector `0x08`.
The original test handler increments r10 and uses `MOVS pc, lr` to restore the caller's status.
The trace shows processor modes, instruction states, CPSR values, and two completed handler calls.
This test image is not a Nintendo BIOS and does not implement BIOS services.

A failed condition skips the instruction without changing registers or flags, except for advancing r15.
For subtraction, carry means no unsigned borrow occurred.
Logical flag-setting operations preserve overflow and take carry from the shifter.
Unshifted operands preserve the previous carry flag.
Arithmetic operations compute carry and overflow from the arithmetic result, not the shifter.

The CPU stores the next instruction address in r15.
In ARM state, ordinary operand reads add eight bytes. Register-specified shifts use a twelve-byte offset.
Storing r15 in ARM state also uses a twelve-byte offset.
Thumb operand reads use PC plus four. Literal loads and PC-relative addresses additionally align down to a word.
`BX` selects the instruction set; ordinary PC writes keep the current instruction set.
On ARM7, Thumb `POP {pc}` remains in Thumb even if the loaded address has bit zero clear.

`Cpu::new` starts in ARM System mode with interrupts enabled. It bypasses hardware boot.
`Cpu::at_reset()` starts at vector zero in ARM Supervisor mode, with IRQ and FIQ disabled.
Both constructors zero registers deterministically; real reset register values are not all defined.
`Cpu::instruction_set()`, `mode()`, `cpsr()`, and `spsr()` report the current processor state.
Each call to `Cpu::step(&mut memory)` executes one ARM word or one Thumb halfword, not one hardware cycle.
The two halves of Thumb `BL` execute separately. A standalone suffix uses the existing link register.

Block transfers assign the lowest-numbered register to the lowest memory address.
They align addresses down without rotating loaded words, and preserve low base bits during writeback.
On ARM7, an empty register list transfers PC but uses a 64-byte span for addressing and writeback.
`LDM` suppresses writeback when the base register is in the list.
With writeback enabled, `STM` stores the old base only when that register is first in the list.
Otherwise, `STM` stores the updated base.

### Processor status and exceptions

User and System share registers. Each exception mode has its own stack pointer, link register, and SPSR.
Fast interrupt mode (`FIQ`) also banks r8–r12. Other modes share those registers.
`MRS` reads status. `MSR` writes selected fields; User mode can only change CPSR arithmetic flags.
Reserved status bits read as zero. Unsupported status fields have no effect.
Invalid CPSR modes and attempts to change its Thumb bit through `MSR` return diagnostics.
Use `BX` or an exception return to change instruction state.

An exception saves CPSR in the destination mode's SPSR and writes that mode's link register.
Entry selects ARM state, masks interrupt requests (`IRQ`), and branches to the exception vector.
FIQ entry also masks FIQ. Ordinary IRQ entry preserves the FIQ mask.
`MOVS pc, lr`, `SUBS pc, lr, #offset`, and `LDM` with S and PC restore CPSR from SPSR.
Return alignment follows the saved instruction state, not target bit zero.
User/System status returns fail because those modes have no SPSR.
An SPSR can contain invalid mode bits, but a return using those bits fails without changing CPU state.

Other S-bit block transfers access User registers while using the current mode's base register.
User-bank writeback, S-bit empty lists, and User-mode S-bit transfers return diagnostics.
These restrictions avoid unpredictable or unverified forms.

`Cpu::enter_exception` explicitly supports software interrupt, undefined instruction, prefetch abort, data abort, IRQ, and FIQ entry.
For synchronous exceptions, call it with PC at the faulting instruction.
For IRQ/FIQ, call it between instructions, with PC at the next instruction.
`Cpu::take_interrupt(irq, fiq)` samples supplied interrupt lines between steps and respects masks and FIQ priority.
`Machine` connects timer, display, and DMA requests to the GBA IRQ line. GBA devices do not generate FIQ.
Memory and unsupported-instruction errors do not automatically enter exceptions.

`Memory::with_bios(rom, bios)` accepts exactly 16 KiB of caller-supplied vector code.
`Memory::new(rom)` leaves the BIOS area unmapped.
Without vector code, `SWI` enters Supervisor mode, then the next fetch reports an unmapped-memory error.
Neither constructor provides Nintendo BIOS services or initializes BIOS-managed RAM.
`bios::boot(rom)` separately opts into the original firmware subset described below.

### Optional original BIOS replacement

`src/bios.rs` builds a deterministic 16 KiB image from original ARM instructions and mathematically generated data.
No Nintendo firmware bytes are included.
A compile-time integer calculation generates the 512-byte sine table at `0x3e00..0x3fff`.
The image builder checks that code and literal pools do not overlap this table.
Literal pools follow unconditional branches and stay within each load instruction's address range.
The CPU executes every instruction through the normal bus, timing, exception, HALT, and DMA paths.
There is no host-side interception of SWIs.

Use `bios::boot(rom)` to create a machine at the reset vector with this image mapped.
Then call `Machine::step()` to execute its minimal boot sequence.
`bios::image()` also exposes the bytes for `Memory::with_bios`; start with `Cpu::at_reset()` to initialize its stacks.
Starting directly at a cartridge with uninitialized banked stacks does not prepare these services.
Existing caller-supplied BIOS images and `Memory::new` behavior remain unchanged.

Minimal boot initializes:

- System stack: `0x03007f00`.
- IRQ stack: `0x03007fa0`.
- Supervisor stack: `0x03007fe0`.
- BIOS IRQ flags at `0x03007ff8` and the callback pointer at `0x03007ffc`: zero.
- IME: zero; POSTFLG: one.

It then enters `0x08000000` in ARM System mode with IRQ/FIQ masks clear.
It does not reproduce Nintendo's logo, cartridge-header checks, RAM clearing, boot delays, or full hardware initialization.
The executable still does not load game files.

Supported software interrupt services:

| Number | Service | Behavior |
| --- | --- | --- |
| `0x00` | SoftReset | Clear BIOS work RAM and reset CPU registers/stacks; restart in ROM or RAM without returning |
| `0x01` | RegisterRamReset | r0 selects RAM and supported I/O resets; force blank; reject serial/sound flags |
| `0x02` | Halt | Wait for `IE & IF`; preserve IME and caller registers |
| `0x04` | IntrWait | `r0`: discard old selected flags when nonzero; `r1`: flags to wait for |
| `0x05` | VBlankIntrWait | IntrWait with discard enabled and the VBlank flag selected |
| `0x06` | Div | Signed `r0 / r1`; return quotient in r0, remainder in r1, and quotient magnitude in r3 |
| `0x07` | DivArm | Div with the incoming numerator and denominator exchanged |
| `0x08` | Sqrt | Unsigned integer square root of r0; return the rounded-down result in r0 |
| `0x09` | ArcTan | Signed fixed-point tangent in r0; return a signed angle in r0 |
| `0x0a` | ArcTan2 | Signed fixed-point X/Y in r0/r1; return the unsigned direction angle in r0 |
| `0x0b` | CpuSet | `r0`: source; `r1`: destination; `r2`: count, fill, and width control |
| `0x0c` | CpuFastSet | Word copy/fill in eight-word blocks; round count upward to a multiple of eight |
| `0x0e` | BgAffineSet | Build background matrices and origins from r0 into r1; r2 is the record count |
| `0x0f` | ObjAffineSet | Build sprite matrices from r0 into r1; r2 is the count, r3 the coefficient stride |
| `0x10` | BitUnPack | Expand packed units from r0 to r1 using the descriptor at r2; write complete words |
| `0x11` | LZ77UnCompWram | Decompress from r0 to r1 with byte writes |
| `0x12` | LZ77UnCompVram | Decompress from r0 to r1 with buffered halfword writes |
| `0x13` | HuffUnComp | Decode 4-bit or 8-bit symbols from r0 to r1 with word writes |
| `0x14` | RLUnCompWram | Expand run-length data from r0 to r1 with byte writes |
| `0x15` | RLUnCompVram | Expand run-length data from r0 to r1 with buffered halfword writes |
| `0x16` | Diff8bitUnFilterWrite8bit | Reconstruct 8-bit samples from r0 to r1 with byte writes |
| `0x17` | Diff8bitUnFilterWrite16bit | Reconstruct 8-bit samples from r0 to r1 with buffered halfword writes |
| `0x18` | Diff16bitUnFilter | Reconstruct 16-bit samples from r0 to r1 with halfword writes |

ARM code uses `SWI service_number << 16`; Thumb code uses the service number directly.
Only User/System callers are supported. Calls from exception modes return a diagnostic rather than risking nested stack corruption.
Returning services preserve CPSR and every caller register except the documented arithmetic outputs.
SoftReset instead initializes CPU registers and status as described below.
This is deterministic behavior, not a claim about undocumented firmware outputs.
Supervisor stack use is 28 bytes normally, including ArcTan and RegisterRamReset. Division and ArcTan2 use 40 bytes.
CpuFastSet, affine-matrix services, BitUnPack, all three decompression formats, and differential filters use 60 bytes.
They do not reproduce the original firmware's internal stack layout or cycle counts.

**IRQ callback contract:** install a word-aligned ARM callback address at `0x03007ffc` after boot.
The dispatcher saves r0–r3, r12, and lr on the IRQ stack, then calls it with r0 equal to `0x04000000`.
The callback must preserve r4–r11, acknowledge handled IF bits, and return with `BX lr`.
For interrupt waits, it must also OR the handled bits into the halfword at `0x03007ff8`.
Acknowledging IF alone does not complete IntrWait. Nested IRQs and SWIs from callbacks are not supported.
The dispatcher restores registers and returns with `SUBS pc, lr, #4`.

IntrWait consumes only the selected BIOS RAM flags; other flags remain set.
With `r0=0`, a previously recorded selected flag permits immediate return.
Otherwise the service enters HALT and permits IRQ callbacks to record new flags.
It masks IME around the flag check and HALT write to avoid losing a wake-up between them.
IME returns enabled, while the caller's CPSR and instruction state are restored.
These routines follow the documented wait contract, not every known firmware implementation quirk.

CpuSet uses the low 21 bits of r2 as its halfword/word count, bit 24 for fill, and bit 26 for word width.
CpuFastSet uses the same count/fill fields but always transfers words using eight-register block loads and stores.
Fill reads its source value once. Zero count performs no source or destination accesses.
Addresses align down to the transfer width; callers should still supply aligned addresses.
Sources below `0x02000000`, or ranges with a wrapping end address, return without copying.
Other invalid accesses use normal memory diagnostics. Earlier successful writes remain committed if a later instruction fails.
These copy services keep CPU IRQ delivery masked until return; device clocks and DMA continue throughout.
BIOS bus protection, exact firmware timing, undocumented side effects, and nested service calls remain incomplete.

#### SoftReset

`SoftReset` (`SWI 0x00`) does not return to its caller.
It reads the byte at `0x03007ffa` before clearing the containing RAM:
zero selects cartridge ROM at `0x08000000`; any nonzero value selects work RAM at `0x02000000`.
Surrounding bytes do not affect this choice. Both destinations start in ARM state, even for Thumb callers.

The service clears exactly `0x03007e00..0x03007fff`, including BIOS IRQ flags, the callback pointer, and the restart flag.
It resets r0–r12 to zero and initializes these CPU banks:

| Mode | Stack pointer | Link register | Saved status |
| --- | --- | --- | --- |
| System/User | `0x03007f00` | Selected restart address | Not present |
| Supervisor | `0x03007fe0` | Zero | Zero |
| IRQ | `0x03007fa0` | Zero | Zero |

Other banked registers and saved status registers remain unchanged.
The service enters System mode with CPSR `0x9f`: IRQ masked, FIQ unmasked, condition flags clear.
This is the subset's deterministic status policy, not independently verified hardware behavior.
It jumps through the System link register and does not execute the cold-boot sequence.

SoftReset does not reset I/O registers or devices.
IME, IE, pending IF bits, POSTFLG, display configuration, and timer configuration remain in place.
Device clocks and DMA continue, so counters and pending requests can change during the service.
IRQ delivery stays masked throughout the service and after restart.
Restart code must install an IRQ callback and configure interrupts before unmasking IRQ.

The common SWI entry still requires an initialized, writable Supervisor stack and saves a 28-byte frame.
SoftReset abandons that frame. It uses no further stack storage and never restores the old caller state.
With default stacks, the frame lies inside the cleared RAM.
If the Supervisor stack is outside this region, its entry writes remain there after reset.
The clearing loop performs ordinary word stores; stopping execution partway leaves a partially cleared region.
Active DMA can still modify memory. Software must stop conflicting transfers before requesting a restart.

Lower internal RAM, external RAM, palette RAM, video RAM, and sprite attribute memory are not cleared by SoftReset.
Use RegisterRamReset or separate initialization code for those regions.
Calls from exception modes retain the existing unsupported-service diagnostic.
Exact firmware timing, bus-protection behavior, and undocumented side effects remain incomplete.

#### RegisterRamReset

`RegisterRamReset` (`SWI 0x01`) selects RAM and supported I/O resets using the low byte of r0.
Bits 8–31 are ignored. The service preserves all caller registers and status.
Supported flags can be combined:

| Bit | Action |
| --- | --- |
| 0 | Clear external RAM: `0x02000000..0x0203ffff` |
| 1 | Clear lower internal RAM: `0x03000000..0x03007dff`; exclude the top 512 bytes |
| 2 | Clear palette RAM: `0x05000000..0x050003ff` |
| 3 | Clear all 96 KiB of video RAM: `0x06000000..0x06017fff` |
| 4 | Clear sprite attribute memory (OAM): `0x07000000..0x070003ff` |
| 5 | Serial reset: unsupported |
| 6 | Sound reset: unsupported |
| 7 | Reset supported display, DMA, timer, and interrupt registers as described below |

**Unsupported flags:** any request containing bit 5 or 6 reaches `bios::INVALID_ARGUMENT_TRAP`.
Validation occurs before forced blank or RAM/I/O reset writes, but after the common SWI stack save.
This includes the common all-devices request `r0=0xff`; it is not silently treated as a successful full reset.
Serial and audio devices remain unimplemented. The firmware's unconditional serial-data side effect is not reproduced.

Every supported request first writes `DISPCNT=0x0080`, including requests with no selected flags.
This forces a white screen and clears other display-control bits.
RAM clears then run in bit order, using ordinary word stores.
Zeroed OAM contains regular sprites at the origin, not disabled sprites; software must configure them before enabling objects.

Bit 7 runs after the selected RAM clears:

- Disable IME.
- Disable all four DMA channels, then zero their source, destination, count, and control registers.
- Stop all four timers, then zero their reload and control registers.
- Zero display registers at `0x04000004..0x04000057`, subject to normal read-only register rules.
- Set BG2/BG3 affine PA and PD to 256; PB, PC, and programmed origins remain zero.
- Clear IE and WAITCNT, then acknowledge IF with a halfword write of `0xffff`.

Timer counter reads retain the stopped count until a later enable loads the cleared reload value.
This is normal timer-register behavior, not a host-side replacement of device state.
Read-only display status and VCOUNT continue to reflect the advancing display clock.
GREENSWAP, KEYINPUT/button state, POSTFLG, and BIOS IRQ communication words are not reset.
Keypad interrupt control is not implemented. Bit 7 covers only the listed supported registers.

Without bit 7, device configuration remains unchanged except for DISPCNT.
IRQ delivery stays CPU-masked during the call. The service restores the caller's mask on return.
Timers and DMA continue until their reset writes, if selected; this service does not stop clocks or replace frame-capture history.
Pending IRQs remain pending without bit 7 and can be delivered after return.

The service uses only the common 28-byte Supervisor frame.
Default BIOS stacks lie in the excluded top 512 bytes, but the entry save still writes its usual frame there.
Do not put the Supervisor stack or required return code/data in selected RAM.
The service can erase a RAM caller's instructions and still return to that now-cleared address.
Stop conflicting DMA transfers before a clear, even when bit 7 is selected: RAM clears occur before device-register resets.
Stopping emulation midway leaves completed stores committed. Exact firmware write ordering and cycle counts are not reproduced.

#### Integer arithmetic

Div truncates the signed quotient toward zero. The remainder has the numerator's sign, or is zero.
DivArm takes the denominator in r0 and numerator in r1, then returns the same outputs as Div.
Both return the unsigned absolute quotient in r3 and preserve r2 and r4–r14.
`INT_MIN / -1` returns `0x80000000` in r0 and r3, with a zero remainder.
Division uses a fixed 32-iteration integer algorithm. It does not use host arithmetic to calculate results.
Division by zero reaches `bios::INVALID_ARGUMENT_TRAP` instead of reproducing the firmware's possible endless loop.

Sqrt treats r0 as an unsigned 32-bit number and returns an unsigned result in `0..65535`.
The result satisfies `root² <= input < (root + 1)²`. Other registers are preserved.
Its integer algorithm uses 16 iterations without floating-point calculations.
Arithmetic services keep IRQ delivery masked until return, without changing IME.
Device clocks and DMA continue. Instruction costs do not reproduce the original BIOS algorithms' timing.

#### Fixed-point angles

`ArcTan` (`SWI 0x09`) and `ArcTan2` (`SWI 0x0a`) accept signed 16-bit fixed-point inputs with 14 fractional bits.
An input value of `0x4000` represents 1.0. This format is also called signed Q2.14.
Inputs must be sign-extended into their 32-bit registers. For example, -1 uses `0xffffffff`, not `0x0000ffff`.
This subset rejects values outside `-32768..32767` through `bios::INVALID_ARGUMENT_TRAP`, rather than interpreting undocumented wider inputs.

Angles use 65,536 units per full turn. A quarter turn is `0x4000`; a half turn is `0x8000`.
ArcTan reads the tangent from r0 and returns a sign-extended signed angle in r0.
For example, tangents +1.0 and -1.0 return +`0x2000` and -`0x2000`, respectively.
The routine evaluates a fixed-point polynomial using low-32-bit products and arithmetic shifts.
Negative intermediate results round down. The implementation does not substitute a host floating-point calculation.
The polynomial has poor accuracy for tangent magnitudes above 1.0; this known limitation is retained.

ArcTan2 reads X from r0 and Y from r1. It returns an unsigned angle in `0..65535` in r0.
It divides the smaller coordinate magnitude by the larger, then applies the shared polynomial and quadrant correction.
The signed ratio truncates toward zero and stays within `[-1.0, 1.0]` before polynomial evaluation.
Axes return exact quarter-turn angles. The zero vector returns zero without division.
Use ArcTan2 for direction calculations that require all four quadrants.

Both services preserve r1–r14 and the caller's status. Undocumented BIOS scratch-register outputs are not reproduced.
They execute original ARM instructions and keep CPU IRQ delivery masked until return, without changing IME.
Device clocks and DMA continue. Exact firmware timing and full hardware compatibility remain unverified.

#### Affine matrices

Both services take a source pointer in r0, destination pointer in r1, and unsigned record count in r2.
They generate inverse-mapping matrices for background and sprite rotation/scaling.
Scale values are signed 8.8: 256 represents 1.0. The services do not calculate reciprocal scales.
Angles use 65,536 units per turn, but only the upper byte is used.

`BgAffineSet` reads 20-byte, word-aligned source records:

| Offset | Field |
| --- | --- |
| 0, 4 | Signed 32-bit texture-center X/Y, with eight fractional bits |
| 8, 10 | Signed 16-bit display-center X/Y, in pixels |
| 12, 14 | Signed 16-bit X/Y scale |
| 16 | Unsigned 16-bit angle |
| 18 | Two padding bytes; not read |

Each word-aligned output record occupies 16 bytes.
It contains four signed halfwords, PA/PB/PC/PD, followed by two 32-bit background origins.
The destination can point directly to BG2 or BG3 affine registers.

`ObjAffineSet` reads eight-byte, halfword-aligned source records.
Offsets 0 and 2 contain signed X/Y scales; offset 4 contains the angle.
The final two padding bytes are not read.
The destination must be halfword-aligned. Register r3 specifies an even coefficient stride of at least two bytes.
Each matrix writes PA/PB/PC/PD at destination offsets `0*stride` through `3*stride`.
The next matrix starts at `4*stride`. Stride two packs coefficients; stride eight preserves intervening sprite attributes in OAM.
Other valid even strides work, and the services leave gaps unchanged.

The shared kernel reads signed sine/cosine values with 14 fractional bits from the generated table.
Products use arithmetic right shifts by 14, then narrow to signed 16-bit intermediates.
PB negates the narrowed X-scale sine product after rounding, not before the shift.
Background origins use those narrowed intermediates and wrap modulo 2³².
All signed scales are accepted, including zero and -32768.
Extreme overflow behavior follows the integer emulator reference; hardware equivalence remains unverified.

Zero count returns before buffer or stride validation, without source or output accesses.
For nonzero counts, invalid alignment, protected sources below `0x02000000`, or wrapping ranges reach `bios::INVALID_ARGUMENT_TRAP`.
Counts cover complete record spans, including padding and the trailing object-stride gap.
Overflow checks run before output writes. They do not require every source byte to be mapped in advance.
Use separate source and output buffers; overlapping buffers are not supported.

The services read each record's fields before writing its output.
A failed source read preserves earlier records without writing the current record.
A failed destination write preserves earlier successful stores, including stores within the current record.
Normal halfword/word bus rules apply to video memory and display registers.
Both services preserve all caller registers and status.
They keep CPU IRQ delivery masked without changing IME; device clocks and DMA continue.
Exact firmware cycle counts and undocumented side effects are not reproduced.

#### LZ77 decompression

Both variants read a word-aligned header at r0. Its low byte must be `0x10`; the upper 24 bits specify output length.
Each flag byte describes eight tokens, most significant bit first.
A literal token produces one byte. A reference token copies 3–18 bytes from 1–4,096 bytes behind the output position.
References can overlap previously produced output. Reads observe each preceding write through the normal emulated bus.
Use separate compressed-source and output buffers; overlapping those buffers is not supported.

The WRAM variant writes bytes and accepts byte-aligned destinations.
Use it only with memory that supports byte writes, such as work RAM. Video/palette RAM byte-write rules still apply.
The VRAM variant also works in work RAM, but always combines two bytes into a halfword store.
This subset requires an even destination address and an even output length for that variant.
It rejects distance-one references, which depend on a previous byte that may still be buffered.
A lone buffered byte is not committed if the next source read fails.
The emulator does not model firmware quirks for odd output sizes or invalid references.

The decoder validates the type byte, source alignment, source protection, output address wrap, and reference bounds.
Sources below `0x02000000`, references before produced output, and runs beyond the declared size reach `bios::INVALID_ARGUMENT_TRAP`.
Invalid VRAM alignment, odd output size, and distance-one references reach the same diagnostic trap.
Zero output length reads no tokens and writes no output after header/argument validation.
Truncated cartridge input and invalid destination accesses return normal memory diagnostics.
Earlier completed writes remain committed on failure; neither the whole service nor the whole output buffer is rolled back.
Each call runs as ordinary ARM instructions, so callers can enforce machine-step limits on large streams.
LZ77 preserves caller registers/status and keeps CPU IRQ delivery masked until return; device clocks and DMA continue.

#### Run-length decompression

Both variants read a word-aligned header at r0. Its low byte must be `0x30`; the upper 24 bits specify output length.
Each block starts with a control byte:

- Bit 7 clear: copy the following `(control & 127) + 1` literal bytes, from 1 to 128 bytes.
- Bit 7 set: read one value and repeat it `(control & 127) + 3` times, from 3 to 130 bytes.

Blocks can alternate freely. Repeated values are read once, rather than read back from the destination.
Decoding stops at the declared output length without reading padding or another block.
Use separate source and destination buffers; overlapping those buffers is not supported.

`RLUnCompWram` accepts byte-aligned destinations and uses normal byte stores. Use memory that supports byte writes.
`RLUnCompVram` buffers bytes across block boundaries and commits complete halfwords, including when the destination is work RAM.
This subset requires an even destination and an even output length for the halfword variant.
A lone buffered byte is not written after a source failure. Odd-size firmware behavior remains unmodeled.

Invalid type bytes, source alignment/protection, output address wrap, and halfword alignment/size reach `bios::INVALID_ARGUMENT_TRAP`.
Sources below `0x02000000` are rejected. Each block must fit the remaining declared output before its payload is read.
An oversized block returns a diagnostic, rather than reproducing firmware behavior on malformed data.
Zero length reads no blocks and writes no output after header/argument validation.
Truncated source data and invalid destination accesses use normal memory diagnostics. Earlier completed writes remain committed.

The CPU executes these routines as original ARM instructions, sharing the byte/halfword output routine with LZ77.
They preserve caller registers/status and keep CPU IRQ delivery masked until return, without changing IME.
Timers, display clocks, and DMA continue during execution. Callers can enforce machine-step limits for large streams.
Exact Nintendo BIOS timing and undocumented side effects are not reproduced.

#### Huffman decompression

`HuffUnComp` (`SWI 0x13`) decodes binary-tree paths into 4-bit or 8-bit symbols.
The source at r0 and the destination at r1 must be word-aligned.
The header's low byte must be `0x24` or `0x28`. Its upper 24 bits specify output length in bytes.
This subset requires a multiple of four output bytes and uses ordinary word stores.

The byte at source offset 4 holds the tree size. The root node follows at offset 5.
The size byte and tree table occupy `2 × (size + 1)` bytes together, including any tree padding.
The compressed bitstream starts immediately after this section. This subset requires that address to be word-aligned; it does not round it.
The table can contain up to 511 bytes after its size byte.

Each internal node uses these fields:

- Bits 0–5 hold a forward child offset.
- The left child address is `(node address & ~1) + 2 × (offset + 1)`.
- The right child follows one byte later.
- Bit 7 marks the left child as a leaf. Bit 6 marks the right child as a leaf.

Input words use little-endian byte order, but branch bits are consumed from bit 31 down to bit 0.
Zero selects the left child; one selects the right child. A leaf contains the decoded symbol.
The decoder returns to the root after each symbol. Internal-node state survives input-word refills.
Output symbols fill each word from its lowest bits upward. For 4-bit output, the first symbol occupies the low nibble.

The decoder validates only visited edges and leaves. It does not scan unused branches or padding.
Traversed children must stay inside the declared table. A visited 4-bit leaf must not contain upper bits.
Invalid headers, widths, alignment, output size, protected sources, wrapping addresses, and invalid tree references reach `bios::INVALID_ARGUMENT_TRAP`.
Sources below `0x02000000` are rejected. These checks are development safeguards, not exact firmware behavior on malformed streams.

Zero output length reads no tree or bitstream after header/argument validation.
Decoding stops at the declared output length without consuming remaining branch bits or trailing words.
Truncated input and invalid stores use normal memory diagnostics. Earlier complete output words remain committed; pending output is discarded on failure.
Use separate source and output buffers. Overlapping buffers and partial final output words are not supported.

The CPU executes original ARM instructions for all traversal and packing. No host-side service hook performs decoding.
The routine preserves caller registers/status and keeps CPU IRQ delivery masked until return. IME is unchanged; device clocks and DMA continue.
Exact Nintendo BIOS timing and undocumented side effects remain unmodeled.

#### Bit unpacking

`BitUnPack` (`SWI 0x10`) expands packed values, such as monochrome font pixels, into wider destination units.
`r0` points to source bytes. `r1` points to word-aligned output. `r2` points to this eight-byte descriptor:

| Offset | Field |
| --- | --- |
| `+0` | 16-bit source length in bytes, from 0 to 65,535 |
| `+2` | 8-bit source unit width: 1, 2, 4, or 8 bits |
| `+3` | 8-bit destination unit width: 1, 2, 4, 8, 16, or 32 bits |
| `+4` | 32-bit offset: bits 0–30 hold the added value; bit 31 enables offsets for zero units |

Source bytes require no alignment. This subset requires a word-aligned descriptor and a destination width at least as large as the source width.
Each source byte supplies its lowest unit first. Output units also fill each word from its lowest bits upward.
The routine adds the offset to nonzero source units. It adds the offset to zero units only when bit 31 is set.
The zero-data flag is not part of the numeric offset.

Output length is `source length × destination width / source width` bytes and must be a multiple of four.
The routine uses ordinary word stores, including for work RAM, video RAM, palette RAM, and OAM.
Use separate source, descriptor, and output buffers. Overlapping these buffers is not supported.

Invalid widths, narrowing, misalignment, incomplete output words, protected sources, and wrapping address ranges reach `bios::INVALID_ARGUMENT_TRAP`.
Sources below `0x02000000` are rejected. Each converted value must fit its destination width after adding the offset.
Overflow produces a diagnostic instead of masking the value or allowing bits to affect neighboring output units.
A failure discards the pending incomplete word; earlier completed stores remain committed.
These checks are development safeguards, not a model of Nintendo BIOS behavior for invalid parameters.

Zero length accesses neither source data nor output after descriptor and argument validation.
Unmapped descriptor/source reads and invalid stores use normal memory diagnostics.
The service preserves caller registers/status and uses original ARM instructions, without host-side SWI interception.
CPU IRQ delivery stays masked until return. IME is unchanged; timers, display clocks, and DMA continue.
Exact firmware timing and undocumented register outputs remain unmodeled.

#### Differential filters

These services reconstruct samples from stored differences. They do not change the payload size.
The source at r0 starts with a word-aligned four-byte header:

- Low byte `0x81` selects 8-bit samples; `0x82` selects 16-bit samples.
- The upper 24 bits specify output length in bytes, including for the 16-bit service.
- The first payload unit is the first original sample. Each following unit is the difference from the previous sample.

The accumulator starts at zero. Each input unit adds to the accumulator, producing the next output sample.
Eight-bit samples wrap modulo 256; sixteen-bit samples wrap modulo 65,536. This handles positive and negative differences.
Sixteen-bit input and output units use little-endian byte order.

`Diff8bitUnFilterWrite8bit` accepts byte-aligned destinations and odd byte counts. It uses ordinary byte stores.
`Diff8bitUnFilterWrite16bit` buffers pairs of reconstructed bytes, then writes halfwords.
`Diff16bitUnFilter` reads and writes halfwords directly. Both halfword-output variants require even destination addresses and byte counts.
Use separate source and output buffers. Overlapping buffers and firmware quirks for odd halfword-output sizes are not supported.

Invalid header types/unit sizes, source alignment, protected sources, output alignment/size, and wrapping address ranges reach `bios::INVALID_ARGUMENT_TRAP`.
Sources below `0x02000000` are rejected. These strict checks are development diagnostics, not exact firmware behavior on invalid data.
After validation, zero output length reads no payload and writes no output. Decoding stops at the declared length without consuming trailing data.
Unmapped source reads and invalid output writes return normal memory diagnostics.
Earlier completed writes remain committed. The buffered byte variant does not write a lone pending byte after a source failure.
All output uses normal bus rules: byte writes to video memory can duplicate bytes, and byte writes to OAM are ignored.

These original ARM routines execute through the CPU, not a host-side service hook.
They preserve caller registers/status and mask CPU IRQ delivery until return, without changing IME.
Timers, display clocks, and DMA continue. Exact Nintendo BIOS timing and undocumented side effects remain unmodeled.

#### Firmware diagnostics

Unsupported SWIs, unsupported exception vectors, and null/misaligned IRQ callbacks reach an intentional undefined-instruction trap.
The CPU reports `CpuError::UnsupportedInstruction` with `bios::UNSUPPORTED_TRAP`, rather than silently treating a service as a no-op.
Invalid arithmetic/decompression arguments use the same CPU error type with the distinct `bios::INVALID_ARGUMENT_TRAP` instruction.
Prior boot/service steps remain committed on failure.
RegisterRamReset serial/sound flags, HardReset, and STOP remain unimplemented.
This subset is not sufficient for Pokémon Emerald compatibility.

### Timers, interrupt registers, and the machine clock

`Machine::new(cpu, memory)` combines the CPU and memory bus.
`Machine::step()` first services one ready DMA data unit, keeping the CPU paused.
If HALT is waiting and no DMA is ready, it advances device clocks to the next event without executing CPU code.
Otherwise, it samples the IRQ line, including the CPU's interrupt mask, then enters IRQ mode or executes one instruction.
Each successful step advances display and timer clocks by its nominal cost.
`Machine::last_timing()` reports separate code, data, internal, and idle cycle totals for the last successful step.
`StepTiming::idle_cycles` counts device-clock cycles during HALT; it does not count CPU work.
IRQ entry is a separate step; the vector instruction runs on the following step.
A device event during a step can trigger IRQ entry once no DMA is ready and CPU masks allow delivery.
Bus writes take effect before the step's device-clock update.
`StepKind::Dma { channel }` identifies a DMA unit. `StepKind::HaltIdle` identifies one bounded HALT clock advance.
The other step kinds remain `Instruction` and `IrqEntry`.
`MachineError::Cpu` and `MachineError::Dma` identify the source of a diagnostic.
Failed steps do not advance the clock or partially change CPU or device state. Earlier successful steps remain committed.

**The clock now uses instruction-specific costs, but it is not cycle-accurate.**
Device clocks update in batches after instructions, exception entries, DMA units, or HALT idle intervals.
Reads observe register state before that batch.
Writes, including timer start/stop and IF acknowledgement, take effect before the entire batch.
This does not reproduce bus-access timing within an instruction or IRQ synchronization delays.

`Cpu::step` remains a CPU-only API. It does not honor HALT, execute DMA, advance device clocks, or sample device IRQs.
`Cpu::step_timed` executes the same operation and returns `StepTiming`, without advancing devices.
`Machine::step` uses that result to advance device clocks. Failed steps preserve the previous timing result.
`Memory::advance_cycles(n)` advances device clocks, latches DMA requests, and updates HALT wake-up state.
It does not execute CPU instructions or DMA units.
Multiple unserviced DMA requests coalesce; bulk clock advances do not replay every missed transfer.
Reads and writes alone consume no cycles.
`Memory::cycles()` and `Machine::cycles()` report the supplied cycle total as a wrapping 64-bit counter.

Mapped input/output (I/O) registers:

| Address | Register | Behavior |
| --- | --- | --- |
| `0x04000100` + `4*n` | Timer n counter/reload | Reads the counter; writes the separate reload value |
| `0x04000102` + `4*n` | Timer n control | Prescaler, count-up, local IRQ enable, and start/stop |
| `0x04000200` | IE | Enables selected interrupt sources; bits 0–13 are writable |
| `0x04000202` | IF | Latches requests; writing 1 clears the corresponding bit |
| `0x04000204` | WAITCNT | Configures first/second ROM access wait states; unused bits read as zero |
| `0x04000208` | IME | Bit zero gates IRQ delivery; other bits read as zero |
| `0x04000300` | POSTFLG | Bit-zero post-boot flag; CPU writes require BIOS execution |
| `0x04000301` | HALTCNT | Write-only low-power control; HALT supported, STOP returns a diagnostic |

Timers count at supplied clock rates divided by 1, 64, 256, or 1024.
Timers 1–3 can instead count the preceding timer's overflows. Timer 0 ignores count-up selection.
Starting a stopped timer copies reload into the counter. Overflow also reloads the counter.
Writing reload while running leaves the counter unchanged until overflow or restart.
A combined 32-bit reload/control write uses the new reload value when starting the timer.
Stopping a timer freezes its counter. Restarting it reloads the counter.
Bulk clock advances preserve all overflow pulses through cascaded timers without looping once per cycle.

Overflow latches IF only when the timer's local IRQ-enable bit is set.
IE, IME, and CPSR.I gate delivery, not the pending flag.
Entering IRQ mode does not clear IF. Software must acknowledge the request.
Unused register bits read as zero. Unknown I/O addresses and mirrors remain unmapped.
Byte, halfword, and word accesses are supported for this register subset.

Timer startup delays and shared prescaler phase are not modeled.
The provisional timer model resets its private prescaler phase on enable or clock-source changes.
Audio events, STOP, and interrupt sources other than timers, display events, and DMA completion are not implemented.

`--timer-demo` executes original ARM code that configures Timer 0, IE, and IME.
The timer overflows after 16 supplied cycles and enters the original handler through vector `0x18`.
The handler stops Timer 0, acknowledges IF, increments r10, and returns with `SUBS pc, lr, #4`.
The demo ends after 21 steps and 153 nominal cycles with one handler call, using default WAITCNT settings.
These counts test the timing model; they are not hardware timing measurements.
Its test handler deliberately changes r2 and r10; it does not implement the Nintendo BIOS calling convention.

### HALT and power-control registers

Writing HALTCNT with bit 7 clear requests HALT. Bits 0–6 are ignored.
The current instruction finishes before the machine pauses CPU execution.
Timers, display timing, and DMA continue to run. HALT does not pause the native window or host input.
`Memory::halted()` and `Machine::halted()` report whether HALT is still waiting.

HALT ends when `IE & IF` becomes nonzero, even when IME or CPSR.I blocks IRQ delivery.
An already pending enabled request prevents the CPU from sleeping.
Wake-up does not acknowledge IF. Clearing IF after wake-up does not put the CPU back into HALT.
On the next machine step, ready DMA still takes priority.
The CPU then enters an unmasked IRQ handler or resumes the next instruction.

Each `HaltIdle` step advances to the earliest display edge or independently clocked timer overflow.
Display edges include every HBlank entry and scanline start. Cascaded timers receive pulses at their predecessor's overflow.
These bounds prevent idle advances from skipping DMA triggers or wake-up events.
They also keep frame execution bounded when no interrupt can wake the CPU.
Idle steps neither fetch instructions nor change CPU registers; code, data, and internal cycle counts remain zero.
Exact hardware entry/exit delays and pipeline effects are not modeled.

CPU writes to POSTFLG and HALTCNT take effect only when the executing instruction address is within the 16 KiB BIOS region.
ARM and Thumb use the same rule. This is an instruction-address check, not a simulated prefetched PC signal.
CPU writes from RAM or cartridge ROM are ignored, including STOP requests.
Bare `Memory::write*` calls remain host/debug setup and can write these registers without a CPU access context.
DMA writes to the power-control block return `DmaError::PowerControlDestination`; this hardware edge case is not modeled.

POSTFLG resets to zero and stores only bit 0. Setting it does not perform a BIOS boot or initialize RAM.
HALTCNT reads return zero as a placeholder, not hardware open-bus behavior.
The reserved bytes at `0x04000302..0x04000303` read zero and ignore writes.
Byte, halfword, and word writes are supported; a halfword/word at POSTFLG also writes HALTCNT.

STOP requests from BIOS code or host setup return `MemoryError::UnsupportedStop` before any write commits.
This prevents STOP from silently behaving like HALT while its clock-gating and wake sources are absent.
Multi-byte and block-store diagnostics preserve POSTFLG, HALT state, CPU registers, and the current step's clock.
CPU-only stepping can record a HALT request but does not enforce the pause. Use `Machine::step()` for device integration.

The graphics demo now uses the optional original BIOS replacement's VBlankIntrWait service.
Its IRQ callback acknowledges VBlank and updates the BIOS RAM flag before the service returns.

### Direct memory access (DMA)

Four DMA channels occupy `0x040000b0..0x040000df`. Each channel has a 12-byte register block:

| Offset | Register | Behavior |
| --- | --- | --- |
| `+0` | SAD | Write-only initial source address |
| `+4` | DAD | Write-only initial destination address |
| `+8` | CNT_L | Write-only halfword/word count |
| `+10` | CNT_H | Read/write address controls, repeat, width, timing, IRQ, and enable |

Byte, halfword, and word register writes are supported. A combined count/control write sets the count before enabling DMA.
Control masks are `0xf7e0` for DMA0–2 and `0xffe0` for DMA3.
Source addresses use 27 bits on DMA0 and 28 bits on DMA1–3.
Destination addresses use 27 bits on DMA0–2 and 28 bits on DMA3.
Initial addresses align down to the selected halfword or word width.
DMA0–2 use 14-bit counts; DMA3 uses 16-bit counts. Zero means 16,384 or 65,536 data units, respectively.
Write-only register reads return zero as a placeholder, not hardware open-bus behavior.

A rising enable bit copies the programmed addresses and count into internal transfer state.
Register writes alone do not move data. `Machine::step()` services one halfword or word at a time.
Source modes are increment, decrement, or fixed. Destination modes also include increment/reload.
Game Pak source addresses increment regardless of the selected source mode.
Writes to programmed addresses/count do not alter an active block's internal pointers or remaining count.
Disable the channel before changing its width, address modes, or start timing; live reconfiguration is not hardware-validated.

Supported triggers:

- Immediate: ready after the enabling instruction; repeat does not keep it running.
- VBlank: ready on entry to line 160.
- HBlank: ready at HBlank entry on visible lines 0–159 only.

Blanking triggers do not depend on DISPSTAT IRQ enables or forced blank.
Enabling during an existing blank period waits for the next edge.
A busy channel ignores additional edges. Pending requests are not a queue.
On repeated blanking transfers, completion reloads the programmed count and optionally the destination.
The source continues from its current position. Non-repeated completion clears the enable bit.
Clearing enable manually cancels a waiting or active transfer.

DMA0 has highest priority, followed by DMA1, DMA2, and DMA3.
A higher-priority request can interrupt a lower-priority transfer between data units.
The CPU remains paused while any channel is ready; display and timer clocks continue.
Completion with local IRQ enable latches IF bits 8–11, independently of IE, IME, and CPSR.I.
CPU delivery waits until no DMA is ready. IRQ entry does not acknowledge IF.

Timing uses independent source and destination bus costs, including WAITCNT and 128 KiB ROM boundaries.
The first unit uses non-sequential accesses plus two internal cycles. Later units use sequential accesses.
Each step reports zero code cycles. Overlapping transfers read each unit after the preceding write; they are not bulk host copies.
Startup scheduling delays, channel-resumption costs, CPU fetch-state effects, and display-bus contention remain unmodeled.

Special timing modes (sound FIFO/video capture), Game Pak DRQ, and prohibited source mode 3 return `DmaError::UnsupportedControl`.
DMA reads below work RAM, including BIOS reads, return `DmaError::UnsupportedSource` rather than exposing BIOS bytes.
DMA writes to DMA registers return `DmaError::RegisterDestination`; self-modifying transfers are not supported.
Other accesses use the existing memory map, alignment checks, mirrors, and I/O write rules.
Unmapped memory and cartridge writes remain diagnostics, not hardware open-bus/latch behavior.
A failed unit changes no memory, device state, or clock. Earlier units remain committed; the failed unit remains pending.

### Display timing, status, and interrupts

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
DMA requested by that VBlank event can still be pending; this method does not wait for a redraw or DMA completion.
A zero or exhausted step limit returns `FrameRunError::StepLimit`.
CPU and DMA diagnostics return `FrameRunError::Cpu` and `FrameRunError::Dma`, respectively.
Completed steps remain committed on error. The method does not inspect demo RAM.
When row capture is enabled, the clock advances within those steps also render visible rows.

### Scanline frame capture

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
Horizontal window history follows the four-cycle comparator model described below, not a complete LCD pixel pipeline.
Vertical window flags update at scanline starts, independently of row capture.
These rules support raster palette, page, scroll, window, and effect changes, but not every hardware raster effect.

The raster demo copies a 256-halfword original color table to `0x02001000` with BIOS `CpuSet`.
During each VBlank, its ARM code polls input, resets the row-zero color, and rearms DMA0.
DMA0 copies one halfword to palette entry zero at each visible HBlank, with repeat and fixed destination enabled.
The source starts at the next row's color. The CPU waits through the visible frame using `VBlankIntrWait` and HALT.
Two debug words at `0x02000000` contain the update count and five-bit color-band phase.
The host neither writes palette colors nor uses those debug words to drive execution.

### Graphics and button input

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

#### Buffered sprite row preparation

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

#### Nominal sprite work allowance

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

#### Mosaic snapshots and vertical counters

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

#### Window masks and color effects

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

#### CPU-driven tile and background demos

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

### Instruction and bus timing

The timing model uses ARM7 cycle summaries:

- **S:** sequential memory access.
- **N:** non-sequential memory access.
- **I:** internal CPU cycle, always one clock cycle.

An ordinary arithmetic instruction costs 1S. A register-specified shift adds 1I, even for a zero shift amount.
Loads cost a code S access, a data N access, and 1I. Stores use N for code and data.
Block transfers charge N for the first data word and S for each following word.
Loads add 1I. Loading PC also adds the branch refill cost.
Branches, PC writes, software interrupts, and exception entry use a nominal 1N+2S code refill.
A skipped conditional instruction only pays its code S access.
Thumb `BL` charges 1S for its prefix and 1N+2S for its suffix.
Multiply costs vary with the incoming multiplier's upper bytes; accumulate and long forms add internal cycles.
ARM uses Rs for this calculation. Thumb multiply uses the incoming destination register.

Data costs use the addresses and widths of actual CPU bus calls, after alignment handling.
BIOS, internal work RAM, OAM, and supported I/O accesses cost one cycle.
External work RAM costs 3 cycles for byte/halfword accesses and 6 for words.
Palette and video RAM cost 1 cycle for byte/halfword accesses and 2 for words, without display contention.
ROM costs include one cycle plus the configured wait states for each 16-bit transfer.
A ROM word uses two halfword accesses; its second halfword always uses sequential timing.
Accesses at 128 KiB ROM boundaries force non-sequential timing for the first halfword.

`WAITCNT` resets to zero and supports byte, halfword, and word access.
The writable mask is `0x5fff`; the Game Pak type flag reads as GBA, and the upper halfword reads as zero.
A CPU write to WAITCNT does not retroactively change that instruction's code-access cost.
The new settings apply to subsequent code accesses.

Important timing limits:

- Code S/N counts follow instruction summaries, not a simulated fetch pipeline.
- Ordinary code costs use the current instruction address. No speculative PC+8 fetch or startup pipeline fill is performed.
- PC writes use destination-region costs and the restored instruction width for nominal refill accesses.
- Refill cost calculation does not read target bytes. An invalid branch target fails on the following instruction fetch.
- Game Pak prefetch is not implemented. WAITCNT bit 14 is stored but does not accelerate execution.
- PHI and SRAM wait fields are stored; PHI output and SRAM mapping are not implemented.
- External work RAM timing is fixed. The undocumented memory-control register is not implemented.
- Exact DMA startup/resumption delays, display-bus contention, shared timer prescaler phase, and timer startup delays remain unmodeled.

## Files

| Path | Purpose |
| --- | --- |
| `flake.nix`, `flake.lock` | Pinned development environment |
| `.envrc` | Automatic environment loading through direnv |
| `src/cpu.rs` | Registers, flags, instruction-set state, and stepping |
| `src/cpu/arm.rs` | ARM decoding, data processing, transfers, branches, and multiply |
| `src/cpu/alu.rs` | Shared arithmetic and barrel shifter |
| `src/cpu/transfer.rs` | Shared ARM/Thumb block transfers and ARM swaps |
| `src/cpu/thumb.rs` | Thumb decoding and execution |
| `src/cpu/status.rs` | Processor modes, register banks, and status transfers |
| `src/cpu/exception.rs` | Reset state, exception entry, and interrupt sampling |
| `src/cpu/status_tests.rs`, `src/cpu/exception_tests.rs` | Status, banking, exception, and return tests |
| `src/cpu/thumb_tests.rs` | Thumb instructions, state switching, and decoder checks |
| `src/cpu/transfer_tests.rs` | Stack, addressing, overlap, empty-list, and swap tests |
| `src/cpu/tests.rs` | Condition truth tables and immediate arithmetic tests |
| `src/cpu/instruction_tests.rs` | Register operations, transfers, branches, and edge cases |
| `src/memory.rs` | Memory mapping, I/O routing, write validation, and device clock |
| `src/io.rs` | I/O registers, timers, HALT wake-up, next-event bounds, and IRQ latches |
| `src/display.rs` | Display clock, scanline status, comparison edges, and display IRQ events |
| `src/dma.rs` | DMA registers, internal pointers, trigger state, priority, and completion IRQs |
| `src/machine.rs` | Timed CPU/DMA/device stepping and IRQ delivery |
| `src/timing.rs` | Bus widths, wait-state costs, and timing breakdowns |
| `src/cpu/timing.rs` | ARM/Thumb cycle summaries and data-access accounting |
| `src/cpu/timing_tests.rs` | Instruction timing and semantic-equivalence checks |
| `src/timer_demo.rs` | Original timer-configuration program and IRQ handler |
| `src/lib.rs` | Core modules and original demo bytes |
| `src/bios.rs` | Original ARM BIOS image builder, minimal boot, IRQ dispatch, waits, and memory services |
| `src/bios/reset.rs` | Emitted ARM SoftReset and selective RegisterRamReset; CPU banks, RAM clearing, and supported device-register resets |
| `src/bios/arithmetic.rs` | Emitted ARM division and integer-square-root routines |
| `src/bios/affine.rs` | Emitted ARM background/sprite matrix services, range checks, and generated sine table |
| `src/bios/angles.rs` | Emitted ARM ArcTan polynomial, ArcTan2 ratio/quadrant handling, and input validation |
| `src/bios/lz77.rs` | Emitted ARM LZ77 decoder with byte and halfword output |
| `src/bios/run_length.rs` | Emitted ARM run-length decoder, header validation, and block bounds |
| `src/bios/bit_unpack.rs` | Emitted ARM packed-unit expansion, offsets, word stores, and validation |
| `src/bios/decompression.rs` | Shared emitted byte/halfword output routine for decompression and byte differential filters |
| `src/bios/differential.rs` | Emitted ARM byte/halfword differential filters, modular accumulation, and validation |
| `src/bios/huffman.rs` | Emitted ARM Huffman tree traversal, symbol packing, word output, and validation |
| `src/video.rs` | Presentation buffer, RGB555 conversion, and Mode 0–5 composition |
| `src/video/affine.rs` | Programmed registers, internal scanline origins, and tiled/bitmap sampling |
| `src/video/windows.rs` | Persistent vertical window edge flags and constant-time clock advancement |
| `src/video/windows/horizontal.rs` | Horizontal comparator flags, per-column history, and bounded bulk advancement |
| `src/video/mosaic.rs` | Live vertical mosaic phase and constant-time counter transitions |
| `src/video/capture.rs` | Drawing/completed frame buffers, row completeness, and deferred render diagnostics |
| `src/video/sprites.rs` | Sprite index preparation, priority metadata, mosaic, late palette lookup, and composition |
| `src/video/sprites/pipeline.rs` | Two prepared row buffers, cycle40 events, and bounded capture-disabled advancement |
| `src/video/sprites/budget.rs` | Nominal per-row work allowance, partial canvas prefixes, and independent arithmetic tests |
| `src/input.rs` | Platform-independent GBA button state |
| `src/graphics_demo.rs` | Original ARM bitmap program and shared bounded frame runner |
| `src/tile_demo.rs` | Original ARM tile/sprite program, palettes, tiles, maps, and OAM image |
| `src/affine_demo.rs` | Shared ARM background program builder and Mode 2 assets |
| `src/affine_raster_demo.rs` | CPU-driven HBlank DMA affine distortion demo |
| `src/bitmap_demo.rs` | Mode 4/5 demo runners, palettes, and original two-page images |
| `src/effects_demo.rs` | CPU-driven window and color-effects demo runner |
| `src/mosaic_demo.rs` | CPU-driven background/sprite mosaic demo runner |
| `src/raster_demo.rs` | Original ARM HBlank DMA raster program and color table |
| `src/video/effects.rs` | Region masks and RGB555 alpha/brightness arithmetic |
| `src/desktop.rs` | Native window, display test, and keyboard controls |
| `src/desktop/affine.rs` | Affine background window and smoke-test pixel checks |
| `src/desktop/affine_raster.rs` | Affine raster window and every-pixel smoke checks |
| `src/desktop/bitmap.rs` | Mode 4/5 windows and page-flipping smoke-test pixel checks |
| `src/desktop/effects.rs` | Window/effects demo and independent smoke-test pixel checks |
| `src/desktop/mosaic.rs` | Mosaic window and 128-frame independent pixel checks |
| `src/desktop/raster.rs` | Raster window and scanline color-band smoke test |
| `src/main.rs` | Command-line modes and fixed-length CPU demo |
| `tests/core.rs` | CPU integration tests and the complete demo |
| `tests/memory.rs` | Memory widths, alignment, errors, and mirrors |
| `tests/exceptions.rs` | Exception demo, BIOS mapping, and reset-vector execution |
| `tests/timers.rs` | Timer rules, interrupt registers, and a cycle-by-cycle reference |
| `tests/machine.rs` | Device IRQ entry/return, clock policy, and I/O failure atomicity |
| `tests/timing.rs` | WAITCNT fields, bus costs, and timer/IRQ timing integration |
| `tests/graphics.rs` | Video memory, rendering, KEYINPUT, and CPU-driven graphics integration |
| `tests/tiles.rs` | Mode 0 registers, maps, palettes, composition, diagnostics, and CPU tile/sprite demo |
| `tests/affine_backgrounds.rs` | Affine registers, maps, transforms, bitmap sampling, composition, and CPU demo |
| `tests/affine_tracking.rs` | Internal origins, mid-frame writes, reloads, mosaic, DMA, and affine raster demo |
| `tests/bitmap_modes.rs` | Mode 4/5 pages, formats, transforms, transparency, sprites, and CPU demos |
| `tests/effects.rs`, `tests/effects/` | Window masks, color arithmetic, target selection, OBJ effects, DMA, and CPU demo |
| `tests/window_tracking.rs`, `tests/window_tracking/` | Vertical/horizontal edges, mid-line writes, hidden positions, DMA, masks, and capture independence |
| `tests/mosaic.rs`, `tests/mosaic/` | Mosaic sizes, sampling, transparent metadata, windows, DMA, and CPU demo |
| `tests/mosaic_tracking.rs` | Size changes, counter wrap, text/affine/OBJ sampling, DMA, and frame resets |
| `tests/scanlines.rs`, `tests/scanlines/` | Capture boundaries, mid-frame changes, DMA ordering, diagnostics, and raster program |
| `tests/sprites.rs` | OAM, DMA, sprite sizes, mapping, palettes, flips, clipping, priorities, and diagnostics |
| `tests/sprite_pipeline.rs` | Preparation boundaries, OAM/VRAM history, late palette/effects, DMA, row0, and diagnostics |
| `tests/sprite_budget.rs` | Regular/affine costs, clipping policy, truncation, inspection, bit5 sampling, DMA, and diagnostics |
| `tests/sprites/affine.rs` | Affine matrices, drawing areas, signed sampling, shared groups, and floating-point reference tests |
| `tests/display.rs` | Display boundaries, register masks, IRQ handlers, and VBlank frame execution |
| `tests/dma.rs` | DMA widths, latches, priority, triggers, IRQs, timing, errors, and CPU integration |
| `tests/halt.rs` | HALT wake masks, idle timing, BIOS-only writes, DMA progress, and STOP diagnostics |
| `tests/bios.rs` | ARM/Thumb service calls, copy/fill boundaries, wait races, callback contracts, and boot |
| `tests/bios/ram_reset.rs` | Selective RAM boundaries, flag combinations, I/O reset, display rendering, DMA/timer latches, and diagnostics |
| `tests/bios/reset.rs` | All restart flags, exact RAM boundaries, CPU banks, restart execution, device continuity, and IRQ masking |
| `tests/bios/arithmetic.rs` | Arithmetic boundaries, wide-integer references, status restoration, and zero-division diagnostics |
| `tests/bios/affine.rs` | Matrix/origin references, all angle phases, strides, live rendering, register preservation, and diagnostics |
| `tests/bios/angles.rs` | Fixed-point references, axes/quadrants, rounding, caller flags, stack bounds, DMA, and diagnostics |
| `tests/bios/lz77.rs` | Token-reference tests, overlaps, output widths, malformed streams, and partial failures |
| `tests/bios/run_length.rs` | All block controls, mixed streams, output widths, status, diagnostics, DMA, and IRQ masking |
| `tests/bios/bit_unpack.rs` | Width pairs, bit references, offsets, maximum length, memory boundaries, status, DMA, and diagnostics |
| `tests/bios/differential.rs` | Round trips, wraparound, output widths, large lengths, partial failures, status, DMA, and diagnostics |
| `tests/bios/huffman.rs` | Independent path encoding, tree bounds, packing, large lengths, partial failures, status, and DMA |

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
Mapped I/O covers DMA, timers, interrupts, WAITCNT, display control/status, background control/scroll/affine registers, window/effect registers, KEYINPUT, POSTFLG, and HALTCNT.
The presentation framebuffer is separate from emulated video RAM.
All named graphics demos connect desktop input and rendering to the emulated CPU. The default display test remains host-generated.
The executable does not accept ROM files yet.

Unmapped reads return errors instead of hardware open-bus values.
Reads beyond the supplied cartridge bytes also return errors.
Direct memory-bus halfword and word accesses require alignment.
CPU load/store instructions apply ARM7TDMI alignment and rotation rules before accessing the bus.
Writes to cartridge addresses return errors instead of modeling cartridge hardware.

## Tests

Run native macOS validation from the project directory:

```sh
direnv exec . cargo fmt --check
direnv exec . cargo clippy --locked --all-targets -- -D warnings
direnv exec . cargo test --locked
direnv exec . cargo test --locked --release
direnv exec . cargo build --locked --release
file target/release/gba-rust
./target/release/gba-rust --smoke-test
./target/release/gba-rust --graphics-smoke-test
./target/release/gba-rust --tile-smoke-test
./target/release/gba-rust --affine-smoke-test
./target/release/gba-rust --affine-raster-smoke-test
./target/release/gba-rust --bitmap4-smoke-test
./target/release/gba-rust --bitmap5-smoke-test
./target/release/gba-rust --effects-smoke-test
./target/release/gba-rust --mosaic-smoke-test
./target/release/gba-rust --raster-smoke-test
./target/release/gba-rust --cpu-demo
./target/release/gba-rust --timer-demo
```

All ten window smoke tests need an active desktop session. Ordinary tests do not open windows.
CPU-driven smoke tests supply scripted buttons; they do not validate physical keyboard events.
To test the core without building the window dependency:

```sh
direnv exec . cargo test --locked --no-default-features
```

This runs 709 core and integration tests; the eleven desktop and command-line tests are excluded.

On Apple Silicon, `file` must report a Mach-O `arm64` executable.
Do not set a Linux cross-compilation target for this validation.

The default suite has 720 tests.
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
Tests cover display/DMA wake sources, DMA priority while halted, BIOS-only writes, STOP diagnostics, and frame limits.
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

## Next steps

1. Validate CPU behavior with public ARM7TDMI test programs before claiming instruction compatibility.
2. Refine nominal timing with a fetch pipeline, Game Pak prefetch, per-access device updates, and verified timer/IRQ delays.
3. Add keypad interrupt control and remaining device registers; extend BIOS reset coverage as sound/serial support becomes available.
   Add STOP and remaining DMA device modes with verified timing and wake-up behavior.
4. Replace nominal sprite work limits with verified individual fetch timing; add background fetch timing and per-pixel composition.
5. Expand graphics, audio, cartridge loading, and saves before testing Emerald compatibility.

Keep the emulator core independent of window and audio libraries.
Treat Nintendo DS support as a separate project phase.

Hardware references used for the core:

- [GBATEK ARM instruction cycle times](https://problemkaputt.de/gbatek-arm-cpu-instruction-cycle-times.htm).
- [GBATEK GBA system control and WAITCNT](https://problemkaputt.de/gbatek-gba-system-control.htm).
- [GBATEK BIOS reset functions](https://problemkaputt.de/gbatek-bios-reset-functions.htm), for SoftReset state and restart selection, plus RegisterRamReset flags, RAM bounds, and forced blank.
- [GBATEK BIOS halt functions](https://problemkaputt.de/gbatek-bios-halt-functions.htm), for wait contracts and STOP differences.
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
- [GBATEK keypad input](https://problemkaputt.de/gbatek-gba-keypad-input.htm).
- [GBATEK memory mirrors and video byte writes](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm).
- [GBATEK GBA memory map and bus widths](https://problemkaputt.de/gbatek-gba-memory-map.htm).
- [GBATEK GBA DMA transfers](https://problemkaputt.de/gbatek-gba-dma-transfers.htm).
- [mGBA DMA implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/dma.c), used to cross-check masks, repeat behavior, and Game Pak source increments.
- [GBATEK GBA timers](https://problemkaputt.de/gbatek-gba-timers.htm).
- [GBATEK GBA interrupt control](https://problemkaputt.de/gbatek-gba-interrupt-control.htm).

- [GBATEK processor registers and modes](https://problemkaputt.de/gbatek-arm-cpu-register-set.htm).
- [GBATEK status transfers](https://problemkaputt.de/gbatek-arm-opcodes-psr-transfer-mrs-msr.htm).
- [GBATEK CPU exceptions](https://problemkaputt.de/gbatek-arm-cpu-exceptions.htm).
- [GBATEK ARM data processing](https://problemkaputt.de/gbatek-arm-opcodes-data-processing-alu.htm).
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
