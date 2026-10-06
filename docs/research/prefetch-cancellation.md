# Prefetch cancellation: independent read observations

## Result

An original ARM probe reproduces the measured structure of the read cases in `zaydlang/PrefetchAbuse`.
Its instruction-boundary cycle differences match every published read observation.
Its actual emulated timer readings now also match, after [ordered timer accesses](ordered-timer-access.md).
The initial comparison isolated a bulk timer-sampling error, not a queue discrepancy.
The original program, published observations, and measurement code remain unchanged.

This is not a run or a port of the upstream ROM.
The expected values are published measurement facts. The program builder and runner are original.
No external implementation, ROM, saved data, Nintendo code, or assets were added.
No physical-hardware measurement or external-emulator differential run was performed locally.

## Pinned evidence

Repository: [zaydlang/PrefetchAbuse](https://github.com/zaydlang/PrefetchAbuse)

Revision: `9ca57c13da7e3c569937f99a42e7c1caca029a2d`

| Reviewed file | Bytes | SHA-256 |
| --- | ---: | --- |
| [README.md](https://github.com/zaydlang/PrefetchAbuse/blob/9ca57c13da7e3c569937f99a42e7c1caca029a2d/README.md) | 1662 | `4ff13a5ec496bcadc65aaa5264703ebafdf0709a7624c1bb1c6175461d213b9c` |
| [src/main.c](https://github.com/zaydlang/PrefetchAbuse/blob/9ca57c13da7e3c569937f99a42e7c1caca029a2d/src/main.c) | 6753 | `0b4286917ae6f7baf9c44012bd521816bcfe7a068df59f12da14fbb8f4e32b04` |

The [Makefile](https://github.com/zaydlang/PrefetchAbuse/blob/9ca57c13da7e3c569937f99a42e7c1caca029a2d/Makefile) was also reviewed for build assumptions.
It expects devkitARM and libgba. This probe does not need either dependency.
The pinned tree contains no license file or built ROM. Its saved-data file was not used.
Only source links, hashes, and factual observations are recorded here; upstream code is not redistributed.

The author describes the expected arrays as hardware results.
The README calls the delays NOPs, but `src/main.c` uses ARM multiply instructions.
The source, not that wording, defines the compared protocol.
The display labels also start at zero, while the first multiply supplies one internal cycle.
The original probe labels cases by actual internal-cycle count, from one through eight.

## Compared protocol

Each case runs twice, once with an additional ROM data read and once without it.
Both runs use fresh machines, the same WAITCNT setting, and the same multiply delay.

1. Set up the timer address, enable value, ROM data address, and multiplier values before measurement.
2. Start Timer 0 with zero reload, prescaler one, and no interrupt request.
3. Execute a multiply with one through four internal cycles.
   For larger delays, execute a four-cycle multiply followed by another multiply supplying the remainder.
4. In the read run only, execute a word load from `0x08000000`.
5. Read Timer 0 with a halfword load.

Code executes in the first ROM wait-state window, away from a 128 KiB boundary.
The final setup literal load cancels earlier queue activity before the timer-start store establishes the measured stream.
All measured instruction bytes and lookahead are mapped.
The probe stops after the timer sample; it does not execute padding, literals, or a return handler.

Our setup register allocation and code layout differ from upstream.
There is no C runtime, console, input loop, BIOS boot, or upstream function prologue in the probe.
The measured store/multiply/read/sample structure and multiplier values are preserved.
This correspondence supports a bounded comparison, not a claim of binary equivalence.

The source compares `read_test - calibration` separately for each delay and WAITCNT setting.
The observations measure that interval difference, not the isolated data-access cost of one LDR.
The four WAITCNT values are `0x4000`, `0x4004`, `0x4010`, and `0x4014`.
The factual read table is recorded in `gba_demos::prefetch_probe::PUBLISHED_READ_DELTAS`.
The queue implementation does not supply or calculate that table.

## Reproduce on Darwin

```sh
direnv exec . cargo test --locked -p gba-core --test prefetch_cancellation
direnv exec . cargo run --locked -p gba-demos --example prefetch_cancellation > /tmp/prefetch-cancellation.csv
```

**The example now exits with status 0 because both comparisons match.**
It previously exited with status 1, exposing the bulk timer-sampling error described below.
A matching instruction-boundary total still cannot override a failed timer comparison.
Status 0 requires both comparisons to match for every case. Status 1 reports mismatches; status 2 reports invalid arguments or execution errors.
The example takes no arguments and needs no downloaded files, window, or external assembler.

Use `--release` to repeat the same checks in a release build.
Debug and release CSV reports match on Darwin arm64.
The CSV columns are:

- `idle_cycles`: sum of multiply internal cycles, not total elapsed time.
- `waitcnt`: configured wait-state control.
- `published_delta`: upstream read-minus-control observation, in decimal cycles.
- `boundary_delta`: read-minus-control machine-clock interval between completed timer-start and timer-sample instructions.
- `timer_delta`: difference between the actual timer values loaded into the CPU register.
- `boundary_matches`, `timer_matches`: separate comparisons with the published observation.

No timing correction is applied to `timer_delta`.
The runner does not hide, patch, or reinterpret failing timer values.

## Historical timer mismatch and correction

Before the correction, the opcode queue saw ordered source, data, and internal costs.
The timer device updated only after the entire instruction completed.
Consequently, the timer load read the old counter before its own source-fetch cost advanced device time.

The read run cancels prefetch with ROM data traffic.
Its subsequent timer load must fetch code from ROM again.
The control run retains queue progress, so its timer load has a shorter code-fetch cost.
Sampling both timers too early therefore loses different numbers of cycles; calibration cannot cancel that difference.

For a one-cycle multiply and WAITCNT `0x4000`:

| Quantity | Cycles |
| --- | ---: |
| Published interval difference | 17 |
| Machine-clock boundary difference | 17 |
| Previous timer-sample difference | 14 |
| Current timer-sample difference | 17 |
| Timer-load source fetch in the read run | 8 |
| Timer-load source fetch in the control run | 5 |

The missing three cycles equal `8 - 5`.
Before the correction, each boundary-minus-timer discrepancy equaled the final source-fetch cost difference.
The sample instructions have identical data and internal costs, so those costs cancel in the subtraction.

With prefetch disabled, the two final source-fetch costs match.
The timer and boundary differences then match each other, and the cancellation-specific extra cycle disappears.
This negative control helps separate the sampling problem from timer subtraction or multiply decoding errors.

Timer data accesses now sample staged device state at bus completion, including the source and data cycles.
Timer writes use the same phase. Successful steps commit that state; failed steps discard it.
The regression now requires actual timer values to match the unchanged published observations.
No offset was added to the loaded value or diagnostic report.
See [ordered timer accesses](ordered-timer-access.md) for start/stop, IF, DMA, API isolation, and rollback coverage.

## Coverage and limits

The extra data cycle appears in the fast sequential settings after four or seven multiply internal cycles.
Those cases match the published completion-edge cancellation pattern.
Original regressions inspect data timing separately from multiply internal work and final timer-fetch timing.
Other checks cover deterministic fresh runs, bounded delay arguments, completion PCs, and disabled-prefetch controls.
Workspace/core/demo tests, clippy, formatting, and rustdoc pass on Darwin arm64 in debug and release.
The existing pinned ARM, Thumb, memory, and BIOS reports remain unchanged.

This comparison does not cover Thumb code, cartridge writes, full-buffer restart, or page crossings.
The upstream write cases remain unsupported because the core reports cartridge writes as diagnostics.
The largest multiply delay here does not establish eight-halfword capacity behavior.
Live WAITCNT changes, DMA, HALT/STOP edges, and exact hardware timer startup delays remain outside this probe.
See [the queue model](gamepak-prefetch.md) for those separate limits.

Ordered timer observations now retain rollback and avoid duplicate device advancement.
Absolute timer startup and control-edge validation remain necessary.
Independent full-buffer and page-boundary measurements remain necessary before claiming complete prefetch timing accuracy.
