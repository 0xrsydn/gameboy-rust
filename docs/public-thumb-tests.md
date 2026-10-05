# Pinned public Thumb test

The headless runner executes the unmodified Thumb ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
It now passes its result checkpoint on Darwin arm64 in debug and release builds. The initial test 229 failure is retained below.
This is an independent compatibility baseline, not complete ARM7TDMI conformance or a hardware measurement.

## Prepare and run

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-thumb --suite thumb
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-public-thumb/suite.json > /tmp/gba-public-thumb-report.json
```

Use a new directory outside the repository or inside ignored `roms/`. The parent directory must exist.
The adapter checks every selected file's byte count and SHA-256 digest before creating output.
It retains the upstream sources, license, README, ROM, generated `suite.json`, and `source-lock.json`.
No source, binary, header, expected result, or instruction is patched or skipped.
Downloaded binaries and header sources must not enter tracked files.

Offline preparation uses the same checks:

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-thumb-offline --suite thumb --source-dir /path/to/gba-tests
```

The existing [ARM command](public-arm-tests.md) remains the default. Explicit `--suite arm` selects the same ARM lock.
Each preparation creates one suite. The suite name does not select the checkpoint's instruction state.
Python comes from Nix and requires no extra packages. Ordinary unit tests use original synthetic data, without downloads.
Preparation is not a compatibility verdict. Inspect the runner's JSON result and exit status.

## Provenance and result protocol

- Repository: `jsmolka/gba-tests`.
- Revision: `a7113b67e63f83a9b321696ddd7042ccfad6c881`.
- Upstream project license: MIT, copyright Julian Smolka; retained with downloaded files.
- ROM: `thumb/thumb.gba`, 3,680 bytes.
- SHA-256: `b5cb2291df4ab314b31c598acd9bff2ccfa0b38efff29daadfe97422ce369b67`.
- Selected file hashes and sizes: [`tools/gba-tests-thumb.lock.json`](../tools/gba-tests-thumb.lock.json).

The adapter uses the upstream committed binary. No FASMARM rebuild or source-to-binary reproduction was performed on Darwin.
The entry code initializes the display, switches to Thumb, and initializes r7 to zero.
Each `m_exit` failure writes its test number to r7 and calls `tmain_end`.
Branch tests also use r7 as a progress marker, then reset it to zero on their successful path.
Successful completion of the memory tests reaches the same `tmain_end` with r7 still zero.

At this revision:

- `tmain_end` is Thumb code at `0x08000930`.
- `ADR r0, eval` (`0xa000`) and `BX r0` (`0x4700`) switch execution back to ARM.
- `eval` is ARM code at `0x08000934`, starting with `0xe92d0003`, the `m_vsync` register save.
- The later result macro reads r7, not the ARM suite's r12.

The source exit paths and binary Thumb `BL` targets were checked against `tmain_end`.
The adapter verifies the bridge and initial evaluation word, in addition to whole-file hashes.
The manifest stops before `eval`, requires ARM state and `r7 == 0`, and allows 1,000,000 successful machine steps.
The budget includes the original BIOS boot. Startup and CPU tests execute normally; result rendering does not execute.
Diagnostics, timeouts, STOP, and nonzero r7 remain failures under the [runner's normal rules](rom-tests.md).

## Observed result on Darwin arm64

Initial integration, before fixing empty-list Thumb stores:

- The runner reaches `0x08000934` after 771 successful steps and 3,858 nominal cycles.
- The CPU is in System/ARM state with CPSR `0x0000001f` and r7 = 229.
- Both builds exit nonzero with `reason: assertion_failed`; there is no emulator diagnostic.
- Test 229 executes `STMIA r0!, {}` (`0xc000`) at `0x080008ba`.
- The stored PC value is `0x080008be`, but the following PC read expects `0x080008c0`.
- The public ARM ROM still passes its unchanged checkpoint.

After correcting the empty-list store PC offset:

- Thumb empty-list stores use executing PC+6 instead of PC+4. ARM stores retain PC+12.
- Test 229 and the later memory tests complete without taking their failure paths.
- The runner reaches `0x08000934` after 807 successful steps and 4,170 nominal cycles.
- The CPU is in System/ARM state with CPSR `0x0000001f` and r7 = 0.
- Debug and release builds exit zero with `reason: checkpoint`, no error, and the result assertion passing.
- Both public ROMs pass with their original bytes, checkpoint addresses, budgets, and assertions unchanged.

Original regressions reproduce the stored-PC mismatch without public ROM bytes.
They cover all low base registers, banked stack pointers, code/data alignment, ROM windows, timing, and atomic diagnostics.
Existing empty-list expectations that incorrectly required PC+4 were corrected; empty loads and ARM stores retain their behavior.
See [CPU transfer semantics](hardware/cpu.md#instruction-demo-and-execution-rules).
This proves only the pinned suites' checkpoints and assertions, not complete CPU, bus, or timing compatibility.
No public timing, memory, BIOS, graphics, or unsafe suite is claimed as passing.
These results do not establish Pokémon Emerald compatibility.

## References

- [Pinned Thumb entry and result flow](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/thumb.asm).
- [Branch progress markers and successful reset](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/branches.asm).
- [Memory tests, including test 229](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/thumb/memory.asm).
- [Shared startup and result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
