# Headless ROM test suites

Use `--test-suite PATH.json` to run repeatable ROM checks without opening a window.
Each case has a machine-step budget, a completion address, and explicit assertions.
A timeout never counts as a pass. Existing `--rom ... --steps ...` remains a diagnostic mode, not a test verdict.

Original programs cover the runner itself. Pinned public [ARM](public-arm-tests.md), [Thumb](public-thumb-tests.md), and [memory](public-memory-tests.md) ROMs provide independent compatibility results.
These pinned ROMs pass their checkpoints on Darwin arm64 in debug and release builds.
The memory ROM has documented assertion and mode-selection limits; a pass is not exhaustive memory validation.
Passing the runner's unit tests does not mean an external suite passes.
A passing suite establishes only its specified checkpoints and assertions. It does not establish Emerald compatibility.

## Run the original smoke suite

```sh
direnv exec . cargo run --locked --example write_test_suite -- /tmp/gba-original-tests
direnv exec . cargo run --locked --release -- --test-suite /tmp/gba-original-tests/suite.json > /tmp/gba-test-report.json
```

The generator creates original ARM, Thumb, and BIOS-division programs, plus a JSON manifest.
The destination directory must not already exist. No Nintendo code, firmware, or game assets are included.
Generated binaries stay outside the repository.
The runner writes one JSON report to stdout. Cargo build messages and errors go to stderr.
Do not redirect the report over the manifest or a ROM file.

## Manifest version 1

```json
{
  "version": 1,
  "cases": [
    {
      "name": "arm-store",
      "rom": "arm.gba",
      "step_limit": 10000,
      "completion": {
        "pc": "0x0800000c",
        "instruction_set": "arm"
      },
      "checks": [
        { "kind": "register", "index": 0, "equals": 42 },
        { "kind": "memory32", "address": "0x02000000", "equals": 42 },
        { "kind": "cpsr", "equals": "0x1f" }
      ]
    }
  ]
}
```

This example matches the generated `arm.gba`, not arbitrary ROMs.
The program stores 42 to external RAM, then reaches an ARM self-branch at `0x0800000c`.

Rules:

- All shown fields are required. Unknown fields and unsupported check kinds are rejected.
- The manifest must be a regular UTF-8 JSON file, no larger than 1 MiB.
- Supply 1–256 cases. Names must be unique, nonempty after trimming, and at most 128 UTF-8 bytes.
- Each case needs 1–256 checks and a nonempty ROM path.
- Relative ROM paths resolve from the manifest's directory, not the current working directory. Absolute paths are accepted.
- Each step limit is a JSON integer from 1 through 100,000,000. Their sum must not exceed 100,000,000.
- Addresses and expected values accept unsigned 32-bit JSON integers, decimal strings, or lowercase `0x`-prefixed hexadecimal strings.
- Signed values, floating-point values, surrounding whitespace, and overflow are rejected. Encode negative bit patterns as unsigned words.
- `instruction_set` is exactly `arm` or `thumb`. ARM completion addresses must be word-aligned; Thumb addresses must be halfword-aligned.
- Use the actual Thumb instruction address, without the address bit used by `BX`.
- Register indices are integers from 0 through 15. Checks read the active register bank.
- `memory32` addresses must be word-aligned. Reads use the host inspection path, including mapping errors.
  BIOS assertions inspect raw mapped bytes, not the protected values seen by CPU code executing outside BIOS.
- `cpsr` compares the complete current program status register. No mask is applied.

The runner validates the whole manifest before opening or executing any ROM.
It uses the same read-only [ROM loader](hardware/cartridge.md#loading-and-boot), with its 4-byte minimum and 32 MiB maximum.
The CLI accepts no other options with `--test-suite`.

## Completion and failure semantics

Each case starts with fresh RAM, devices, CPU state, and the original BIOS replacement.
The budget includes BIOS boot, instructions, DMA units, IRQ entries, and HALT idle batches.
A failed CPU or DMA step does not consume a successful step or advance the reported state.
The runner supplies no input and does not enable frame capture. Device clocks still advance through normal machine stepping.

After successful progress, the runner checks PC and instruction set at machine-step boundaries.
It stops before executing the instruction at the completion address.
Reaching that address on the final allowed step succeeds if all checks pass.
The initial reset state alone cannot pass a case. STOP always ends the case as a failure because no input source can wake it.
HALT continues advancing devices until completion, a diagnostic, or budget exhaustion.

The checkpoint does not imply that pending DMA or IRQ work has completed.
Choose an address reached only after the tested operations finish.
A loop address reached before those operations would define an invalid test, even if the runner reports a pass.
The runner does not infer program completion from an infinite loop, a screen, or matching register values alone.

All checks run at the checkpoint. A mismatch or unreadable memory address fails the case.
Cases that never reach the checkpoint do not evaluate their checks.
A failure does not skip later cases. No machine state passes from one case to another.

The budget bounds machine steps, not elapsed host time. Use an external process timeout in continuous integration if needed.
This is not a filesystem security sandbox. Run trusted suite configurations and use files you may lawfully use.

## Report version 1 and exit status

Top-level fields:

| Field | Meaning |
| --- | --- |
| `format_version` | Report schema version, currently 1 |
| `bios` | `original`; no external BIOS option |
| `passed` | True only when every case passed |
| `case_count`, `failed_count` | Number of executed cases and failures |
| `cases` | Results in manifest order |

Each case records its name, ROM path, debug-formatted resolved path, byte count, budget, completion condition, and result.
`state` contains registers r0–r15, PC, CPSR, mode, instruction set, HALT/STOP flags, nominal cycles, and step counters.
Addresses, registers, expected values, and counters in reports are JSON numbers, even when the manifest used hexadecimal strings.
A load failure has null `state` and `rom_bytes`.
`checks` contains the configured check, actual value, pass flag, and optional read error when the checkpoint was reached.
Otherwise `checks` is empty. A failed read has a null actual value.

Case reasons are:

- `checkpoint`: completion reached and all assertions passed.
- `assertion_failed`: completion reached but at least one assertion failed or could not read memory.
- `step_limit`: completion was not reached within the budget.
- `stopped`: STOP prevented further execution.
- `emulation_error`: an instruction or DMA diagnostic stopped execution; `error` contains its message.
- `load_error`: loading or machine creation failed; `error` contains its message.

Exit status 0 means all cases passed and the report was written successfully.
Any failed case, invalid configuration, or output failure returns a nonzero status.
Invalid manifests produce a stderr diagnostic without a JSON report. A broken output stream can leave an incomplete report.
Valid suites produce a report even when some ROMs fail to load.
Reports omit host timestamps and durations so unchanged files produce repeatable output on the same host.
They do not hash ROM contents; record source revisions and file checksums alongside external test results.

## Next validation stage

The first public integration is [jsmolka/gba-tests ARM](public-arm-tests.md), with a verified result-register checkpoint.
The pinned ARM ROM now passes this checkpoint on Darwin arm64. Historical failures remain documented.
The [pinned Thumb ROM](public-thumb-tests.md) also passes its verified r7 checkpoint after the empty-list store correction.
The [pinned memory ROM](public-memory-tests.md) also passes, without a core change and with explicit coverage limits.
The [pinned BIOS read-protection ROM](public-bios-tests.md) reaches its checkpoint but fails test 1 with the original replacement BIOS.
Its expected values depend on Nintendo firmware. Record that failure rather than changing the assertion or returning guessed constants.
For further public ARM7TDMI/GBA tests, inspect their source, license, entry assumptions, and result protocol.
Pin source revisions and build instructions before comparing emulator changes.
Adapt suites to verified completion addresses and result locations; do not guess them or treat timeouts as passes.
Some tests need debug-port logging, input, different firmware behavior, or rendered-image checks that this runner does not support yet.
Keep external binaries local. Record consulted sources in [references.md](references.md).
