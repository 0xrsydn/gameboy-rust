# Pinned public BIOS read-protection test

The headless runner executes the unmodified BIOS test ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
It currently fails test 1 on Darwin arm64 in debug and release builds.
This ROM tests protected BIOS reads against Nintendo firmware opcode values, not the complete BIOS service set.
Our runner uses the original replacement BIOS. It neither supplies nor loads Nintendo firmware.

## Prepare and run

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-bios --suite bios
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-public-bios/suite.json > /tmp/gba-public-bios-report.json
```

`bios/bios.gba` is a public test cartridge, not a BIOS image.
Use a new directory outside the repository or under ignored `roms/`. Its parent directory must exist.
The adapter checks every selected file's size and SHA-256 digest before creating output.
It retains the upstream license, source, ROM, `suite.json`, and `source-lock.json` without modifying them.
Downloaded binaries and header sources must not enter tracked files.

Offline preparation uses the same checks:

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-bios-offline --suite bios --source-dir /path/to/gba-tests
```

ARM remains the default suite. The Thumb and memory selections remain unchanged.
Preparation is not a passing result. The BIOS run currently exits nonzero and writes a failed assertion report.

## Provenance and checkpoint

- Repository: `jsmolka/gba-tests`.
- Revision: `a7113b67e63f83a9b321696ddd7042ccfad6c881`.
- Upstream project license: MIT, copyright Julian Smolka; retained with downloaded files.
- Test ROM: `bios/bios.gba`, 1,908 bytes.
- SHA-256: `9d7b369fa1aa661ff03692b3d79c6f644b623d72983d0fc890e6d87a0409a3c9`.
- Selected file hashes: [`tools/gba-tests-bios.lock.json`](../tools/gba-tests-bios.lock.json).

The adapter uses the upstream committed binary. No FASMARM rebuild or source-to-binary reproduction was performed on Darwin.
The source initializes r12 to zero. Each failure sets r12 to its test number and branches to `eval`.
Successful execution reaches the same label with r12 still zero.
The source exits and binary ARM branch targets were checked against `eval` at `0x08000248`.
Its first instruction is `0xe92d0003`, the shared `m_vsync` register save.
There is another `m_vsync` inside test 3 at `0x08000194`; that is not the completion checkpoint.

The manifest requires ARM state at `0x08000248` and `r12 == 0`, within 1,000,000 successful machine steps.
The budget includes original BIOS boot. All test setup executes normally; only final result rendering is excluded.
Test 3's VBlank wait and IRQ callback are part of the test and are not bypassed.
Preparation verifies the checkpoint word in addition to whole-file hashes.
Timeouts, diagnostics, STOP, and nonzero r12 remain failures under the [normal runner rules](rom-tests.md).

## Observed result on Darwin arm64

Before BIOS read protection:

- Both builds reach `eval` after 64 successful steps and 543 nominal cycles.
- The CPU is in System/ARM state, with CPSR `0x2000001f` and r12 = 1.
- The report says `reason: assertion_failed`, with no emulator diagnostic.
- Test 1 reads address zero after boot. It expects `0xe129f000`, a Nintendo BIOS opcode.
- The emulator instead exposes the replacement BIOS reset-vector instruction at address zero.
- The later post-SWI and IRQ checks are not reached.

This reveals a missing bus-protection rule and a separate firmware-specific expectation.
Implementing protection must not substitute Nintendo opcode constants merely to pass this ROM.
The original BIOS has a different layout, so its retained prefetch data can differ even with protection implemented.
No public BIOS pass is claimed. The ARM, Thumb, and memory suites remain separate passing results.
Exact fetch-pipeline behavior and full BIOS compatibility remain unverified.

## References

- [Pinned BIOS test source and expected opcodes](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/bios/bios.asm).
- [Shared initialization and result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [GBATEK BIOS read protection](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
