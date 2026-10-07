# Emerald starter-selection diagnostic: empty LZ77 input

## Report and isolation

A user reported a failure while selecting one of the three starters in a local Emerald run.
The window reported 37,101 captured frames before an intentional replacement-BIOS argument trap:

```text
PC=0x00000fd4 CPSR=0x80000093 Supervisor/Arm
r0=0x08d66484 r1=0x020057dc r2=0x00000000
r11=0x00000000 r13=0x03007fa4 r14=0x082e7092
unsupported instruction 0xe7f000f1 at 0x00000fd4
```

`0xe7f000f1` is our `bios::INVALID_ARGUMENT_TRAP`, not an unimplemented ARM opcode from the game.
The printed instruction address in the user's final error line was truncated; the CPU state supplied the full address.
The run selected `--rtc --save-type flash128 --window --audio` but omitted `--save-file`.
Its save memory was therefore volatile. This diagnosis does not recover progress from that exited process.

Local read-only inspection found Thumb `SWI 0x11` at the reported link register minus two.
That selects `LZ77UnCompWram`. Our decoder increments r0 by four when reading the header.
The inferred header address, `0x08d66480`, contains `0x00000000` in the locally supplied ROM.
The declared output length is zero.

A temporary, original ARM caller reproduced the service in isolation:

1. Map the local ROM unchanged with our original BIOS image.
2. Install an original caller in IWRAM and initialize its Supervisor stack.
3. Call `SWI 0x11` with source `0x08d66480` and destination `0x020057dc`.
4. Put a sentinel at the destination and bound execution to 1,000 machine steps.

Before the fix, this reproduced the same trap PC and r0/r1/r2/r11 values.
The preceding instruction was at `0x00000c28`, the invalid-header conditional branch.
After the fix, the isolated call returned after 78 machine steps with the destination sentinel unchanged.
The probe did not boot through the game or reconstruct its entire live state.
No game code, graphics, firmware, ROM modifications, or save data were added to the repository.
The reproduction applies to the local file; no cartridge revision was inferred from its filename.

## Cause and bounded correction

The decoder required header byte `0x10` before examining the output length.
An empty all-zero header therefore failed validation, although it needed no token reads or output writes.

Both LZ77 variants now read the header and check its upper 24-bit length first.
A zero length returns through normal register/status restoration, independent of the low header byte or destination.
Source alignment, source protection, and the header read still apply.
Nonempty streams retain the existing type, output alignment, address-wrap, distance, and run-length checks.
Other decompression services are unchanged. No ROM address or game identity selects special behavior.

GBATEK documents the standard `0x10` LZ77 header and its upper 24-bit length.
It does not establish all malformed-header behavior or physical zero-length timing.
The pinned mGBA implementation extracts the length, assumes the signature, and decodes only while the length is positive.
That provides emulator precedent for an empty-stream return, not a hardware measurement.
We did not copy its implementation or broaden acceptance of nonempty invalid-type streams.
Our replacement BIOS remains original ARM code and does not reproduce exact firmware timing or register clobbers.

## Validation and remaining check

Original regression tests failed before the correction and pass afterward.
They cover every low byte of an empty header, both services, ARM/Thumb callers, and caller-register/status restoration.
They preserve RAM, video, and stack sentinels.
Headers ending at ROM EOF and unmapped odd destinations establish that neither tokens nor output are accessed.
Existing nonempty malformed/truncated stream diagnostics still pass.
Original process and native-window tests call both services and verify return markers and unchanged destination data.

Workspace debug/release tests, strict Clippy, formatting, and warning-free Rust documentation pass on Darwin arm64.
Pinned public ARM/Thumb/memory/BIOS suites and Pong scenarios also pass in both profiles; Pong reports match.
Full starter selection and subsequent gameplay after this fix require user retesting.
The isolated result does not establish Emerald save compatibility or a complete playthrough.

## References

- [GBATEK BIOS decompression](https://problemkaputt.de/gbatek-bios-decompression-functions.htm): standard LZ77 header, length, output widths, and reference format.
- [mGBA BIOS at 3a5e34be](https://github.com/mgba-emu/mgba/blob/3a5e34be33dc7f8f707e5bc9db69e8a430046f21/src/gba/bios.c): `_unLz77` extracts the upper 24-bit length and uses `while (remaining > 0)` without checking the low signature byte.
- [Rust gba LZ77 wrapper documentation](https://docs.rs/gba/latest/gba/bios/fn.LZ77UnCompReadNormalWrite8bit.html) and [gbadoc BIOS overview](https://gbadev.net/gbadoc/bios.html): secondary format/calling references found during research, not evidence for physical empty-header behavior.
