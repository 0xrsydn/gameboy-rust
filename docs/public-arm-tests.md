# Pinned public ARM test

The headless runner now executes the unmodified ARM ROM from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests).
This is independent public test code, not a complete ARM7TDMI conformance suite or a new hardware measurement.
The full public ARM test still fails. The first discovered CPU error is fixed; the next unsupported instruction is recorded below.

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
Preparation does not mean the test passed. The current run exits nonzero and writes a failed JSON report.

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

After the fix:

- Execution progresses past tests 234 and 235 and the PSR-transfer section.
- After 861 successful steps, the runner reports unsupported instruction `0xe5b00004` at `0x0800132c`.
- This is `LDR r0, [r0, #4]!` in upstream test 360, with base and destination equal.
- The CPU is in System/ARM state, and r12 remains zero. The final completion checkpoint has not been reached.

The load/writeback alias case is the next compatibility task. Do not weaken the assertion or label this run a pass.
No public Thumb, timing, memory, BIOS, graphics, or unsafe suite has been claimed as passing.
These results do not establish Pokémon Emerald compatibility.

## References

- [Pinned ARM entry and result flow](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/arm.asm).
- [Pinned test/result macros](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/lib/macros.inc).
- [Tests 234 and 235](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/data_processing.asm).
- [Load/writeback alias tests](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/arm/single_transfer.asm).
- [Pinned license](https://github.com/jsmolka/gba-tests/blob/a7113b67e63f83a9b321696ddd7042ccfad6c881/LICENSE).
