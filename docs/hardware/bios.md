# BIOS replacement

The original BIOS replacement built by `crates/gba-core/src/bios/` and the services it implements.

`crates/gba-core/src/bios.rs` builds a deterministic 16 KiB image from original ARM instructions and mathematically generated data.
No Nintendo firmware routines or dumped image are included. Documented protected-read words are explicit compatibility data.
A compile-time integer calculation generates the 512-byte sine table at `0x3e00..0x3fff`.
The image builder checks that code and literal pools do not overlap this table.
Literal pools follow unconditional branches and stay within each load instruction's address range.
The CPU executes every instruction through the normal bus, timing, exception, HALT/STOP, and DMA paths.
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
- BG2/BG3 affine PA and PD: 256 (identity scale), through executed ARM halfword stores.
  Fresh device state supplies zero PB, PC, and origins. Raw `Memory::new` defaults remain unchanged.

This prevents bitmap and affine ROMs from sampling one pixel across the screen when they rely on firmware initialization.
Original regressions verify bitmap coordinates, both affine backgrounds, and scanline capture without ROM-side matrix writes.
See the [boot graphics investigation](../research/boot-video.md) for the independent homebrew failure and evidence.

It then enters `0x08000000` in ARM System mode with IRQ/FIQ masks clear.
It does not reproduce Nintendo's logo, cartridge-header checks, RAM clearing, boot delays, or full hardware initialization.
The executable can load raw ROM files for [terminal or window execution](cartridge.md) using this boot sequence.
This does not establish game compatibility or complete BIOS behavior.

Supported software interrupt services:

| Number | Service | Behavior |
| --- | --- | --- |
| `0x00` | SoftReset | Clear BIOS work RAM and reset CPU registers/stacks; restart in ROM or RAM without returning |
| `0x01` | RegisterRamReset | r0 selects RAM and supported I/O resets, including disabled sound and disconnected serial state |
| `0x02` | Halt | Wait for `IE & IF`; preserve IME and caller registers |
| `0x03` | Stop | Freeze system clocks until an enabled live keypad condition wakes the system; preserve caller state |
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

## CPU BIOS read protection

CPU data reads at `0x00000000..0x00003fff` depend on the executing instruction's address, not its processor mode.
While executing inside BIOS, ARM and Thumb code can read the mapped image directly.
While executing outside BIOS, both instruction sets read byte lanes from retained BIOS prefetch data instead.
Normal word rotation, odd-halfword behavior, sign extension, and load timing still apply.

The current interpreter implements bounded ARM and Thumb BIOS prefetch snapshots:

- Before an ARM BIOS instruction executes, use its pipeline's new mapped PC+8 fetch without charging another data access.
- Before a Thumb BIOS instruction executes, sample the full mapped word at `(PC+4) & ~3`.
  BIOS drives a 32-bit word even for a halfword fetch; both distinct lanes are retained.
- After successful execution, retain that snapshot for later protected reads. Skipped conditional instructions count as successful execution.
- Instructions outside BIOS do not replace the retained value. Re-entering ARM or Thumb BIOS code refreshes it.
- Diagnostic failures preserve the previous value and clear the CPU access context.
- Unavailable lookahead bytes invalidate the known value after a successful BIOS instruction.
- A protected read without known history returns `MemoryError::Unmapped` at the requested bus address, rather than exposing raw BIOS bytes.

The initial history is unknown. Direct cartridge startup with `Cpu::new` does not manufacture boot prefetch data.
Use BIOS execution, such as `bios::boot`, to establish supported history.
All retained words come from the supplied image. The bus never substitutes a firmware-specific constant.
Our generated image places documented compatibility data at PC+8 of its non-fallthrough exits:

| Boundary | Retained word |
| --- | --- |
| Boot and SoftReset | `0xe129f000` |
| SWI return | `0xe3a02004` |
| IRQ callback entry | `0xe25ef004` |
| IRQ return | `0xe55ec002` |

Boot, SoftReset, SWI return, and IRQ return skip a diagnostic trap at PC+4 and the data word at PC+8.
The callback branch instead has the real `SUBS pc,lr,#4` return instruction at PC+8.
Builder fixups preserve this layout without adding executed instructions or host-side service hooks.
Other supplied BIOS images retain their own values; their data is not rewritten.
See the [root-cause research](../research/bios-readback.md) for evidence and the firmware-versus-bus distinction.

Host/debug reads and ROM-suite memory assertions still inspect raw mapped bytes and do not initialize or change the history.
Instruction fetches remain strict mapped reads outside the CPU data-access context.
BIOS writes remain read-only diagnostics. DMA BIOS sources never read image bytes.
They reuse known [channel data](dma.md#retained-channel-data), or report an unsupported-source diagnostic when channel data is unknown.
Missing BIOS images remain unmapped. The unused-memory ARM open-bus snapshot is separate from this retained BIOS value.

The Thumb rule follows documented BIOS bus lanes and a source comparison with NanoBoyAdvance.
See [Thumb BIOS prefetch research](../research/thumb-bios-prefetch.md) for evidence, emulator differences, and test scope.
The BIOS latch remains separate from the CPU's ARM/Thumb instruction buffer.
Both now consume a shared fetch sample; BIOS history no longer rereads lookahead memory at execution entry.
It does not model exact refill, BIOS data-access, or DMA bus history.
The boundary diagnostic policy and precise bus timing remain hardware-unverified.
Original regressions cover synthetic images, boot, exception returns, access widths, alignment, modes, and diagnostics.
The [public BIOS ROM](../public-bios-tests.md) passes its unchanged protected-read assertions in debug and release builds.
This result covers these firmware boundaries, not full BIOS service compatibility.

## Stop

`Stop` (`SWI 0x03`) executes an original ARM byte store of `0x80` to HALTCNT.
It uses the common 28-byte Supervisor frame and restores caller registers/status after wake-up.
It leaves IE, IME, KEYCNT, and DISPCNT unchanged.
Configure KEYCNT and IE bit 12 before calling; no matching wake source means the system stays stopped.
The caller can force blank separately before entering STOP.
Both ARM and Thumb User/System callers are supported.

A keypad wake does not set IF in the stopped-clock model.
The BIOS resumes its return sequence without requiring an IRQ callback or BIOS IRQ-flag update.
Ready DMA and previously pending IRQs follow the ordinary machine scheduling rules after wake-up.
See [STOP behavior and limits](timers-irq.md#stop-with-keypad-wake-up).
Serial/Game Pak wake-up and exact oscillator restart timing are not implemented.

## SoftReset

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

## RegisterRamReset

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
| 5 | Reset supported idle serial state and select general-purpose inputs |
| 6 | Reset disabled sound state, mixing control, bias, and accessible wave RAM |
| 7 | Reset supported display, DMA, timer, and interrupt registers as described below |

All low-byte flag combinations, including `r0=0xff`, are accepted within the supported device subset.
This is not full audio or serial support. [Sound activation](audio.md) and [serial transfers](serial.md) still fail explicitly.
The firmware always clears the low halfword of SIODATA32, even when bit 5 is clear; its upper halfword is preserved.

Every supported request first writes `DISPCNT=0x0080`, including requests with no selected flags.
This forces a white screen and clears other display-control bits.
RAM clears then run in bit order, using ordinary word stores.
Zeroed OAM contains regular sprites at the origin, not disabled sprites; software must configure them before enabling objects.

After the RAM clears, bit 5 clears SIOCNT and SIODATA8, writes RCNT=0x8000, and clears supported Joybus reset registers.
Disconnected general-purpose inputs read high, so RCNT reads back as 0x800f.
Bit 6 writes SOUNDCNT_X=0, SOUNDCNT_H=0, and SOUNDBIAS=0x0200, then clears the accessible wave RAM bank.
Disabled PSG registers remain zero. The other wave bank cannot be enabled in the current subset and remains zero.
Reset does not produce audio, fabricate serial transfers, or bypass normal bus accesses.

Bit 7 runs after the selected RAM and sound/serial resets:

- Disable IME.
- Disable all four DMA channels, then zero their source, destination, count, and control registers.
- Stop all four timers, then zero their reload and control registers.
- Zero display registers at `0x04000004..0x04000057`, subject to normal read-only register rules.
- Set BG2/BG3 affine PA and PD to 256; PB, PC, and programmed origins remain zero.
- Clear IE and WAITCNT, then acknowledge IF with a halfword write of `0xffff`.

Timer counter reads retain the stopped count until a later enable loads the cleared reload value.
This is normal timer-register behavior, not a host-side replacement of device state.
Read-only display status and VCOUNT continue to reflect the advancing display clock.
GREENSWAP, KEYINPUT/button state, KEYCNT, POSTFLG, and BIOS IRQ communication words are not reset.
Bit 7 covers only the listed registers. A later matching input sample can request keypad IRQ again after IF acknowledgement.

Without their corresponding reset flags, device configuration remains unchanged except for DISPCNT and the SIODATA32 low halfword.
IRQ delivery stays CPU-masked during the call. The service restores the caller's mask on return.
Timers and DMA continue until their reset writes, if selected; this service does not stop clocks or replace frame-capture history.
Pending IRQs remain pending without bit 7 and can be delivered after return.

The service uses only the common 28-byte Supervisor frame.
Default BIOS stacks lie in the excluded top 512 bytes, but the entry save still writes its usual frame there.
Do not put the Supervisor stack or required return code/data in selected RAM.
The service can erase a RAM caller's instructions and still return to that now-cleared address.
Stop conflicting DMA transfers before a clear, even when bit 7 is selected: RAM clears occur before device-register resets.
Stopping emulation midway leaves completed stores committed. Exact firmware write ordering and cycle counts are not reproduced.

## Integer arithmetic

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

## Fixed-point angles

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

## Affine matrices

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

## LZ77 decompression

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

## Run-length decompression

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

## Huffman decompression

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

## Bit unpacking

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

## Differential filters

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

## Firmware diagnostics

Unsupported SWIs, unsupported exception vectors, and null/misaligned IRQ callbacks reach an intentional undefined-instruction trap.
The CPU reports `CpuError::UnsupportedInstruction` with `bios::UNSUPPORTED_TRAP`, rather than silently treating a service as a no-op.
Invalid arithmetic/decompression arguments use the same CPU error type with the distinct `bios::INVALID_ARGUMENT_TRAP` instruction.
Prior boot/service steps remain committed on failure.
Active sound, serial transfers, HardReset, and serial/Game Pak STOP wake-up remain unimplemented.
This subset is not sufficient for Pokémon Emerald compatibility.
