# Pinned public BIOS read-protection test

The headless runner executes the unmodified BIOS test ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
It passes its unchanged protected-read assertions on Darwin arm64 in debug and release builds.
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
Preparation is not a passing result. A successful BIOS run exits zero and reports `r12 == 0` at the verified checkpoint.

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

### Historical failures

Before BIOS read protection:

- Both builds reach `eval` after 64 successful steps and 543 nominal cycles.
- The CPU is in System/ARM state, with CPSR `0x2000001f` and r12 = 1.
- The report says `reason: assertion_failed`, with no emulator diagnostic.
- Test 1 reads address zero after boot. It expects `0xe129f000`, a Nintendo BIOS opcode.
- The emulator instead exposes the replacement BIOS reset-vector instruction at address zero.
- The later post-SWI and IRQ checks are not reached.

After the bounded ARM BIOS read-protection implementation:

- Both builds still report test 1 as failed after 64 steps and 543 nominal cycles.
- The protected read now returns `0xe59f1320`, sampled from the original BIOS at its final boot instruction's PC+8.
- It no longer exposes address zero's reset-vector word, `0xea000006`.
- The test still expects `0xe129f000`. No assertion, checkpoint, ROM byte, or returned constant was changed to force a pass.
- Tests 2–4 remain unreached. Their SWI and IRQ cases have not passed independently.

### Current passing result

After correcting the replacement firmware's exit layout:

- Both builds reach `eval` at `0x08000248` after 17,606 successful steps and 197,456 nominal cycles.
- The CPU is in System/ARM state, with CPSR `0x6000001f` and r12 = 0.
- The report says `reason: checkpoint`, with no emulator diagnostic and one IRQ entry.
- Boot, returning Sqrt, external IRQ callback, and IRQ return checks all execute and pass.
- The ROM, checkpoint, r12 assertion, and step budget remain unchanged.
- Debug and release reports match exactly for the same fixture path.

The initial failure exposed both a missing bus-protection rule and a firmware layout difference.
The protected-read bus still derives its retained word from the actual supplied image.
Our generated firmware now places documented compatibility words at its exit instructions' PC+8 locations.
These words are skipped data, not copied firmware routines. No bus override or ROM-specific condition was added.
The IRQ callback uses its existing real return instruction as the readback word.
See the [root-cause research](research/bios-readback.md) and [CPU BIOS read-protection limits](hardware/bios.md#cpu-bios-read-protection).

The ARM, Thumb, and memory suites remain separate passing results.
This BIOS result covers boundary readback values, not complete BIOS services, exact fetch history, or commercial-game compatibility.
Separate original tests cover bounded [Thumb BIOS snapshots](research/thumb-bios-prefetch.md); this public ROM does not validate that path.
Full pipeline behavior, physical-hardware Thumb readback, and exact firmware timing remain unverified.

## References

- [Pinned BIOS test source and expected opcodes](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/bios/bios.asm).
- [Shared initialization and result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [GBATEK BIOS read protection](https://problemkaputt.de/gbatek-gba-unpredictable-things.htm).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
