# Local Emerald input probe: wave trigger

## Claim and method

The earlier 600-frame native run established execution without a diagnostic, not gameplay.
This probe checks whether scheduled input changes captured output and exposes another unsupported operation.
The game remains a local user-supplied file. No ROM bytes, assets, screenshots, or extracted instructions are committed or submitted to Jev.

A temporary host driver uses public core APIs:

1. Boot the local ROM with `bios::boot` and select `CartridgeHardware::Rtc` and `SaveDevice::Flash128`.
2. Set deterministic RTC components to `[24, 1, 1, 1, 12, 0, 0]`. Do not advance this clock during the probe.
3. Enable scanline rendering and call `Machine::run_until_vblank(400000)` once per requested frame.
4. Apply each input snapshot after the named VBlank, before the next machine step.
5. Require `present_frame` to return a complete image. Stop on any execution/render diagnostic.
6. Compare captured pixels locally against a fresh no-input run with identical hardware/time configuration.

The initial budget is 900 VBlanks. Each frame has its own machine-step bound; this is not the CLI's total step budget.
A follow-up extends the limit to 1,320 VBlanks. The temporary driver rejects requests above 1,500 VBlanks.
These are local diagnostic runs, not committed ROM-suite assertions or a commercial compatibility test.
The current manifest versions cannot select this cartridge hardware, so `--test-suite` cannot reproduce this setup directly.

| VBlank | Buttons held after the event |
| --- | --- |
| 0 | None |
| 600 | Start |
| 610 | None |
| 720 | Start |
| 730 | None |
| 840 | Start |
| 850 | None |
| 960 | A, extended run only |
| 970 | None |
| 1080 | A, extended run only |
| 1090 | None |
| 1200 | A, extended run only |
| 1210 | None |

## Observed failure

Before wave support, the first Start press changed captured output.
Execution then stopped before frame 638:

```text
PC=0x082e104c
successful steps through frame 637: 62597533
unsupported PSG channel trigger (synthesis not implemented): write 0x86 at 0x04000075
SOUND3CNT_L readback: 0
SOUND3CNT_H readback: 0
```

The wave gate was disabled. A trigger in this state must not activate channel 3.
Rejecting every trigger was an overly strict feature limit, not evidence that the game needed audible wave output at that moment.
The correction implements general [single-bank wave behavior](../hardware/audio.md#wave-channel-3-single-bank-playback), including the inactive trigger case.
It does not patch this game's instruction or skip its write.

## Result after the correction

The unchanged initial schedule reaches 900 frames and 89,199,123 successful machine steps without a diagnostic.
The extended schedule reaches 1,320 frames and 128,588,887 successful machine steps, ending at PC `0x080008ca`.
The no-input control also reaches 900 frames without a diagnostic.

Local pixel comparisons against that control show:

| Captured frame | Different pixels out of 38,400 |
| --- | --- |
| 600, before input | 0 |
| 620 | 38,400 |
| 660 | 38,232 |
| 900 | 38,400 |

This establishes input-dependent execution and different captured output under fixed device configuration.
It does not establish readable menus, correct scene rendering, release behavior inside the game, or gameplay.
Local optical character recognition did not identify reliable menu text and supplied no semantic acceptance criterion.
No visual classifier or Jev call was used. No title-screen claim follows from color counts or pixel differences.
The file's cartridge revision was not inferred from its filename.

Original regressions separately test active wave synthesis, nibble rotation, lengths, gains, gating, bus phases, and rollback.
Workspace debug/release tests, public ARM/Thumb/memory/BIOS suites, pinned Pong scenarios, and native Darwin checks pass.
The pinned Pong debug/release reports match. Native ROM-window tests include an original wave-activation program.
A separate 600-frame Emerald native run still reaches its frame limit without input or a diagnostic.

## Remaining requirements

- Verify visible menu text and gameplay through direct window testing or explicit independently justified assertions.
- Fixed-rate capture and opt-in Darwin `--audio` output were added later. Verify listening quality and reduce observed underruns.
- Research two-bank wave playback and live bank changes separately. Both remain diagnostic during active playback.
- Implement Flash programming/erase and safe persistence before claiming saving.

For a manual native check, use:

```sh
direnv exec . cargo run --locked --release -- --rom roms/pokemon-emerald.gba --rtc --save-type flash128 --window
```

Enter maps to Start. Z maps to A. Escape exits. Add `--audio` for optional Darwin output.
This command does not supply the deterministic probe schedule or fixed RTC.
Hardware sources and implementation comparisons are recorded in [audio references](../hardware/audio.md#references).
