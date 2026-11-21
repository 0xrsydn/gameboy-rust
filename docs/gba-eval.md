# GBA Eval

[GBA Eval](https://github.com/mechanize-work/gba-eval) grades from-scratch GBA emulators built to its
WebAssembly ABI. It runs each candidate against Mesen 2 on 27 testcases: procedural test ROMs judged by their final
frame, and gameplay replays of homebrew games judged frame by frame. Scores combine per-frame structural similarity of
the picture (60% replays, 20% procedural) and log-mel similarity of the audio (20%).

`crates/gba-eval` builds this emulator as the benchmark's candidate, `gba_emu.wasm`. Local scoring runs the same code
natively against [Mesen](mesen-reference.md), configured as the benchmark configures its reference.

## Build

The ABI requires the benchmark's BIOS stub at address 0. The stub is MIT licensed, but it is not original to this
repository, so it is never committed. Fetch it, with the corpus, from the pinned archive (`tools/gba-eval.lock.json`
pins commit `e39dc39` by size and SHA-256; every ROM is checked against the hash `testcases.json` records):

```sh
direnv exec . python3 tools/prepare_gba_eval.py roms/gba-eval
GBA_EVAL_BIOS=$PWD/roms/gba-eval/gba_bios_stub.bin direnv exec . \
  cargo build --locked --release --lib --target wasm32-unknown-unknown -p gba-eval
```

The result is `target/wasm32-unknown-unknown/release/gba_emu.wasm` (about 210 KB). It imports nothing and exports the
ten ABI functions, plus `emu_boot_frames`. The dev shell provides `lld`, which Rust uses to link WebAssembly. Without
`GBA_EVAL_BIOS` the build embeds this emulator's own firmware instead, which the benchmark does not accept.

The benchmark builds its candidates from the root of its own task container (`/task`, package `gba_emu`). Submitting
there means copying `gba-core` and this crate into that layout, with the stub embedded; that is not automated here.

## How the candidate lines up with the reference

The benchmark's reference is its `reference/mesen_step.cpp` driving Mesen. `Session` reproduces each choice that affects
what a frame shows:

| Reference | `gba_emu::Session` |
| --- | --- |
| `SkipBootScreen = true`: Mesen starts at the cartridge without running any boot code | `bios::skip_boot`: the same CPU, I/O, open-bus, and display state, with the stub mapped for SWIs and IRQs; see [BIOS](hardware/bios.md#skipped-boot) |
| Burns 6 frames before frame 0, and exports `emu_boot_frames` = 6 when run as a candidate | Exports `emu_boot_frames` = 6 |
| A frame ends at the PPU's entry to line 160 | `Machine::run_until_vblank`, then the captured frame |
| Input is polled at that VBlank, so keys set before a frame apply as it ends | Keys are latched and applied after the frame's VBlank |
| Save type: three forced game codes, then the first of `SRAM_V`, `EEPROM_V`, `FLASH_V`, `FLASH512_V`, `FLASH1M_V` anywhere in the ROM, else SRAM | `gba_emu::save_device`, the same rules |
| 32768 Hz stereo audio, drained per frame | Captured 10-bit levels scaled by 64 to i16, 32768 Hz |

Running the stub from its reset vector instead reaches the same frames (Spout changes picture at the same frame), but
leaves different I/O state; `skip_boot` matches Mesen exactly where that state is visible. With these choices, Spout's
first 300 frames without input are pixel-identical to the reference, both natively and in the built `.wasm` run under
Node.js.

A core diagnostic (unsupported hardware behavior) stops the machine: later frames keep the last picture and the audio
stops. `Session::error` reports it; the ABI has no error channel.

## Local scoring

`gba-diff --eval` runs one testcase. This emulator runs through `Session`; mesen-ref runs in its `eval` mode, which
loads as the benchmark's reference does. Both burn the boot frames, take the replay's keys before each frame, and
compare pictures after 5-bit quantization. Audio is not compared. `--images FRAME,... --out DIR` writes those frames
(ours, Mesen's, and a difference mask) as PPM files.

```sh
direnv exec . cargo build --locked --release -p gba-diff
target/release/gba-diff --reference roms/mesen-reference/mesen-ref --eval \
  --bios roms/gba-eval/gba_bios_stub.bin --rom roms/gba-eval/corpus/roms/homebrew/spout.gba \
  --replay roms/gba-eval/corpus/replays/spout-gameplay.txt --frames 4475
direnv exec . python3 tools/score_gba_eval.py          # every testcase, in parallel
```

`score_gba_eval.py` judges `endstate` testcases by their last frame and replays by the share of identical frames. That is
stricter than the benchmark's structural similarity, which tolerates small differences, so it is a proxy for finding
differences, not the benchmark's number.

## Results

2026-11-22, this emulator at `15b3068` plus this work, on Darwin arm64:

- Endstate testcases: 13 of 16 final frames identical. `aw-arm-ldm-stm` stops on an unsupported block transfer,
  `dma-priority` on a DMA transfer that writes DMA registers, and `ppu-vram-mirror` differs from frame 0.
- Replays: 48,627 of 53,366 frames identical (91.1%). Celeste, piugba, and Collie Defense match on every frame or all
  but one. Spout drifts from frame 344 (22% identical); Heartwrench, Waimanu, and Chip Advance each have a stretch of
  frames that differ across the whole screen.
- Many testcases differ only in a few pixels of their first compared frame.

## Limits

- Audio is not compared locally; mesen-ref does not expose Mesen's samples.
- Mesen enables an RTC (on the host clock) for ROMs with `SIIRTC_V001`; `Session` has none. No corpus ROM has one.
- The benchmark's metric is structural similarity with per-replay thresholds; this proxy counts identical frames.
