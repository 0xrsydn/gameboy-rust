# BIOS protected-read mismatch: root cause and fix

## Finding

The remaining public BIOS failure comes from the replacement firmware's exit layout, not its protected-load byte lanes.
The memory bus correctly returns the replacement image's retained ARM PC+8 word.
That word differs from the documented value left by the standard GBA firmware.
Matching the bus mechanism alone does not make replacement firmware's observable outputs compatible.

At the initial failing checkpoint:

- The unmodified public ROM expects `0xe129f000` after boot.
- Our replacement's final boot instruction is at `0x60`; its PC+8 word at `0x68` is `0xe59f1320`.
- The returned value matches that image word, rather than the reset vector or ROM caller's prefetch.
- The ROM sets r12 to 1 and reaches its real evaluation checkpoint. This is an assertion failure, not a CPU diagnostic.

The original regression `original_boot_protected_read_matches_its_actual_last_bios_snapshot` verifies this image-derived behavior.
It computes the expected word from the executed boot trace, not from a Nintendo opcode constant.

## Evidence

[GBATEK](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm) documents PC-dependent BIOS protection and the most recently fetched BIOS opcode.
For standard ARM firmware exits, the observed word comes from the exit instruction's PC+8 location.
The [pinned public test](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/bios/bios.asm) checks these lifecycle values:

| Firmware boundary | Standard source address | Protected-read word |
| --- | --- | --- |
| Boot / SoftReset exit | `0x0dc + 8` | `0xe129f000` |
| SWI return | `0x188 + 8` | `0xe3a02004` |
| Inside external IRQ callback | `0x134 + 8` | `0xe25ef004` |
| IRQ return | `0x13c + 8` | `0xe55ec002` |

The addresses describe standard firmware. Our replacement does not need the same internal addresses to expose the same values.

[Smolka's Progress Report #6](https://www.smolka.dev/posts/progress-report-6) directly compares original and replacement BIOS return prefetch.
It traces a Pokémon Ruby/Sapphire null-pointer read to this firmware-dependent value.
The report notes that Emerald fixed that particular game bug. It does not establish Emerald compatibility for our emulator.
The report also shows that different firmware values can be valid under the bus rule while affecting software behavior.

[mGBA's software BIOS implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/bios.c) explicitly sets `biosPrefetch` to `0xe3a02004` after a returning SWI.
Its [memory implementation](https://github.com/mgba-emu/mgba/blob/master/src/gba/memory.c) instead retains fetched BIOS data when executing a mapped BIOS image.
These are different implementation paths for the same observable behavior. Our core uses executed ARM firmware, not intercepted SWIs.

The [Cult-of-GBA replacement BIOS](https://github.com/Cult-of-GBA/BIOS/tree/a30e9a96df083628b650724b7d4d7112b4070b98) was also reviewed.
Its original assembly and MIT license illustrate the separate roles of firmware routines and their surrounding image layout.
No replacement BIOS code, binary, artwork, or Nintendo BIOS image was copied into this repository.

## Implementation decision

Make the original generated firmware expose the documented lifecycle readback values through its real image layout.
Do not add a bus override, game identifier, ROM hash check, result-register write, or public-test patch.
Do not change caller-supplied BIOS images or their generic protection behavior.

Reserve two words immediately after each relevant non-fallthrough exit:

1. Put an intentional diagnostic trap at exit PC+4.
2. Put the documented compatibility word at exit PC+8.

The exit skips both words. The normal ARM prefetch snapshot observes the second word.
These words are explicit compatibility data in the generated image, not executable copies of firmware routines.
The builder's normal fixups relocate branches and literals around them.
Cold boot and SoftReset need the reset word. Returning SWIs and IRQs need their respective words.

The IRQ callback path already has the real `SUBS pc, lr, #4` instruction at the callback branch's PC+8.
Keep that executed return instruction in place and test the layout invariant separately.

This corrects the earlier overly broad claim that reproducing any documented opcode value would merely force a test pass.
Arbitrarily overriding a read to satisfy an assertion would be wrong.
Reproducing documented firmware boundary outputs for all callers is a legitimate compatibility feature, provided its scope is explicit.

## Validation requirements and limits

- Run the pinned BIOS ROM unchanged, with its original budget, checkpoint, and r12 assertion.
- Test boot, SoftReset, returning SWIs, IRQ callbacks, and IRQ return with original programs.
- Verify byte lanes, load widths, caller state, banked stacks, and ARM/Thumb returns.
- Verify that the compatibility words are never executed during normal exit paths.
- Keep synthetic caller-supplied images image-derived, including images with different exit words.
- Rerun public ARM, Thumb, and memory suites and native Darwin regressions.

This change does not implement a persistent instruction pipeline, Thumb BIOS fetch history, or exact firmware timing.
It does not prove complete BIOS compatibility or commercial-game support.
The public BIOS ROM checks these protected-read values; it is not a comprehensive BIOS service suite.
