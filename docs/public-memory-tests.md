# Pinned public memory test

The headless runner executes the unmodified memory ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
It passes its result checkpoint on Darwin arm64 in debug and release builds without a core behavior change.
This suite samples memory mirrors and video-byte writes. It does not establish complete memory-bus compatibility.

## Prepare and run

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-memory --suite memory
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-public-memory/suite.json > /tmp/gba-public-memory-report.json
```

Use a new directory outside the repository or inside ignored `roms/`. Its parent directory must exist.
Every selected source and binary file has a pinned byte count and SHA-256 digest.
The adapter checks all files before creating output, retains the upstream license, and writes `suite.json` and `source-lock.json`.
No ROM patch, source rewrite, header replacement, test skip, or result adjustment occurs.
Downloaded ROMs and header sources must not enter tracked files.

Offline preparation applies the same checks:

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-memory-offline --suite memory --source-dir /path/to/gba-tests
```

The default remains the [ARM suite](public-arm-tests.md). `--suite thumb` selects the [Thumb suite](public-thumb-tests.md).
Each invocation prepares one suite. Normal Rust and Python tests do not download public files.
Python comes from Nix and requires no extra packages. Preparation alone is not a compatibility verdict.

## Provenance and result protocol

- Repository: `jsmolka/gba-tests`.
- Revision: `a7113b67e63f83a9b321696ddd7042ccfad6c881`.
- Upstream project license: MIT, copyright Julian Smolka; retained with downloaded files.
- ROM: `memory/memory.gba`, 2,172 bytes.
- SHA-256: `21024fb6aae6343f5f0466dd54e3149de1fbeb23f78e7d85a015c983684d2f87`.
- Selected file hashes and sizes: [`tools/gba-tests-memory.lock.json`](../tools/gba-tests-memory.lock.json).

The adapter uses the committed upstream binary. No FASMARM rebuild or source-to-binary reproduction was performed on Darwin.
Startup initializes Mode 4 display state and sets r12 to zero.
Failure paths set r12 to their test number and branch to `eval`.
Successful execution reaches `eval` with r12 still zero.
The source exits and their binary ARM branch targets were checked against this address.

`eval` is `0x08000350`, in ARM state. Its first word is `0xe92d0003`, the shared `m_vsync` register save.
Preparation checks this word in addition to whole-file hashes.
The manifest stops before executing `eval` and requires `r12 == 0`.
This runs startup and all memory tests without depending on result rendering, VBlank polling, or BIOS number formatting.
The budget is 1,000,000 successful machine steps, including original BIOS boot.
Diagnostics, timeouts, STOP, and failed assertions remain failures under the [runner's normal rules](rom-tests.md).

## Observed result on Darwin arm64

- The ROM reaches `0x08000350` after 174 successful steps and 1,478 nominal cycles.
- The CPU is in System/ARM state, with CPSR `0x6000001f` and r12 = 0.
- Debug and release builds exit zero with `reason: checkpoint`, no error, and the result assertion passing.
- The existing ARM and Thumb suites also retain their passing results.
- No emulator behavior change was needed for this result.

## Coverage and limits

The mirror tests sample external RAM, internal RAM, palette RAM, video RAM, object attribute memory, and both alternate cartridge windows.
The byte-write tests sample palette duplication, background video RAM duplication, and ignored object-memory writes.
They do not cover open bus, BIOS protection, save hardware, unused I/O, memory timing, or display-bus contention.

The pinned source also has limits that weaken its video-byte checks:

- Tests 50–52 reject one exact byte-sized value, rather than requiring the complete prior word to remain unchanged.
  An incorrect duplicated-halfword write can therefore escape these checks.
- Startup selects Mode 4. Test 51 applies `ORR #3` to that mode, selecting reserved Mode 7 rather than a supported bitmap mode.
- Test 52 clears only mode bits 0–1. This leaves Mode 4 selected rather than selecting a tile mode.
- Test 5 explicitly selects Mode 0 for its upper video RAM mirror check. It does not validate every mirror in every mode.

These source behaviors are retained, not patched to make the suite stronger or easier to pass.
Passing this ROM must not be described as exhaustive video-memory validation.

Original CPU-driven regressions in `crates/gba-core/tests/video_bus_cpu.rs` provide separate, stronger byte-write checks.
They select each supported mode explicitly and require the exact duplicated halfword or unchanged nonzero sentinel.
They cover both byte lanes, physical and mirrored addresses, tile/bitmap boundaries, forced blank, and CPU readback.
Adjacent halfwords must remain unchanged. Ignored stores still pay their nominal data-access cost and advance devices.
Halfword and word stores still modify object video RAM and object attribute memory.
These are implementation regressions based on documented rules, not new hardware measurements.

No public BIOS, timing, graphics, save, or unsafe suite is claimed as passing.
Pokémon Emerald compatibility remains unverified.

## References

- [Pinned memory entry and result flow](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/memory.asm).
- [Memory mirror checks](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/mirrors.asm).
- [Video-byte checks and mode selection](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/memory/video_strb.asm).
- [Display initialization](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/text.asm) and [shared result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [GBATEK memory mirrors and video-byte writes](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
