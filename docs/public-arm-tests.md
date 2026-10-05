# Pinned public ARM test

The headless runner now executes the unmodified ARM ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
This is independent public test code, not a complete ARM7TDMI conformance suite or a new hardware measurement.
The pinned public ARM ROM now passes its result checkpoint on Darwin arm64 in debug and release builds.
Compare/status, load/writeback aliases, and ARM unused-memory reads are fixed. Earlier failures are retained below.

## Prepare and run

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-arm
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-public-arm/suite.json > /tmp/gba-public-arm-report.json
```

The destination must not exist. Its parent directory must exist.
Use a directory outside the repository or within the ignored `roms/` directory.
The adapter downloads pinned files over HTTPS and checks each byte count and SHA-256 digest before creating output.
It retains the upstream MIT license, README, ARM sources, shared assembly includes, and the original ROM.
It also writes `suite.json` and `source-lock.json`. The manifest references the ROM relative to its own directory.
No source rewriting, header removal, binary patching, or expected-result adjustment occurs.

Python comes from the existing pinned Nix environment. No Python packages are required.
The adapter bounds each download, sets a network timeout, and refuses existing output directories.
A disk-write failure can leave partial output; choose a new destination for another attempt.
Ordinary Rust and Python unit tests do not download or execute external ROMs.

For an offline copy of the selected upstream files:

```sh
direnv exec . python3 tools/prepare_gba_tests.py /tmp/gba-public-arm-offline --source-dir /path/to/gba-tests
```

Offline input must match every selected file in the lock. A directory from a different revision can fail verification.
The adapter does not overwrite or modify the input tree.

## Provenance and checkpoint

- Repository: `jsmolka/gba-tests`.
- Revision: `a7113b67e63f83a9b321696ddd7042ccfad6c881`.
- Upstream project license: MIT, copyright Julian Smolka. The license is retained with downloaded files.
- ROM: `arm/arm.gba`, 8,824 bytes.
- SHA-256: `77ee88662552bdc885c1080c0172ff119d54db791bd73b21808cf1ff1fe5b40e`.
- All selected file hashes and sizes: [`tools/gba-tests-arm.lock.json`](../tools/gba-tests-arm.lock.json).

This adapter uses the committed upstream binary, not a local rebuild.
Upstream documents FASMARM as its assembler. No FASMARM build or source-to-binary reproduction was performed on Darwin.
The binary's console header includes logo bytes. Do not copy the downloaded binary or header source into tracked files.
Only original adapter code and provenance metadata are committed here.

The ARM source initializes r12 to zero before testing.
Each failure path sets r12 to its test number and branches to `eval`.
Successful execution falls through to the same label with r12 still zero.
At this pinned revision, `eval` is `0x08001d4c`, in ARM state.
The ROM word there is the initial register save of the `m_vsync` macro.
The source failure exits and their encoded branch targets were inspected against that address.

The manifest stops before executing `eval` and requires `r12 == 0`.
This avoids depending on the result renderer, VBlank polling, or BIOS division used to print failure numbers.
It does not bypass the ROM's startup code or any CPU tests.
In particular, evaluation must stop before using a possibly invalid stack left by a failed mode-switch test.
The instruction at the checkpoint is checked during preparation, in addition to whole-file hash verification.

The case budget is 1,000,000 successful machine steps, including our original BIOS boot.
Timeouts, emulator diagnostics, and nonzero r12 all remain failures under the [normal suite rules](rom-tests.md).
Preparation does not mean the test passed. Check the runner's exit status and JSON assertions separately.

## Observed results on Darwin arm64

Before the compare/status fix:

- The ROM reaches `eval` after 579 successful steps.
- The assertion reports r12 = 234, in FIQ mode.
- Running beyond `eval` produces an unmapped stack access because the CPU incorrectly remains in FIQ mode.
- Upstream test 234 uses an ARM compare encoding with unused destination bits set to R15.

The CPU previously treated that encoding as an ordinary compare and only changed arithmetic flags.
It now restores CPSR from the current mode's SPSR when one exists, without writing a result to PC.
User/System forms still use ordinary test/compare flags because those modes have no SPSR.
Original regressions cover all four test/compare operations, exception banks, flags, masks, operands, timing, conditions, and invalid saved modes.
See [CPU status behavior](hardware/cpu.md#processor-status-and-exceptions).

After the compare/status fix, before load/writeback alias support:

- Execution progresses past tests 234 and 235 and the PSR-transfer section.
- After 861 successful steps, the runner reports unsupported instruction `0xe5b00004` at `0x0800132c`.
- This is `LDR r0, [r0, #4]!` in upstream test 360, with base and destination equal.
- The CPU is in System/ARM state, and r12 remains zero. The final completion checkpoint has not been reached.

After load/writeback alias support, before ARM unused-memory reads:

- Tests 360 and 361 complete without taking their failure paths.
- After 877 successful steps, the runner reports `unmapped memory at 0x80000000`.
- The current instruction is `0xe7b12060` at `0x08001384`: `LDR r2, [r1, r0, RRX]!`, in test 362.
- With carry set and r0/r1 zero, the shifted offset correctly selects `0x80000000`.
- The unsupported bus read prevents completion. r12 remains zero, but that alone is not a passing result.

Single-load aliases now retain the loaded value rather than the updated base.
Original regressions also cover byte, halfword, and signed loads, every non-PC register bank, index modes, offsets, alignment, timing, and failures.

After ARM unused-memory open-bus support:

- Test 362 completes. The read returns the ARM word at the executing PC plus eight, not a high-address mirror.
- Execution also reaches the later halfword alias and block-transfer tests without taking their failure exits.
- The runner reaches `eval` at `0x08001d4c` after 1,344 successful steps, including original BIOS boot.
- The CPU is in System/ARM state, with CPSR `0x6000001f`, r12 = 0, and 11,257 nominal cycles.
- Debug and release runs exit zero, with `reason: checkpoint`, no error, and the result assertion passing.
- The pinned ROM, manifest checkpoint, budget, and expected result are unchanged.

Test 362 checks shifted addressing, writeback, and carry, but does not check the loaded value.
Original regressions separately check the returned word, byte lanes, rotation, sign extension, timing, and retained diagnostics.
See the bounded [ARM open-bus implementation](hardware/cpu.md#arm-unused-memory-data-reads).
This is a passing result for this pinned ARM ROM, not proof of complete instruction, memory, or timing compatibility.
The [pinned public Thumb ROM](public-thumb-tests.md) also passes its verified checkpoint.
The [pinned public memory ROM](public-memory-tests.md) passes its limited mirror and video-byte checks.
The [public BIOS read-protection ROM](public-bios-tests.md) has a separate bounded passing result.
No public timing, graphics, save, or unsafe suite has been claimed as passing.
These results do not establish Pokémon Emerald compatibility.

## References

- [Pinned ARM entry and result flow](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/arm.asm).
- [Pinned test/result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [Tests 234 and 235](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/data_processing.asm).
- [Load/writeback alias and shifted-offset tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/single_transfer.asm).
- [Halfword load/writeback alias expectations](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/halfword_transfer.asm).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
