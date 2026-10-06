# AGENTS.md

Orientation for coding agents. This is a map, not a rulebook: use your judgment, and read the linked doc for the area you are touching rather than everything.

## What this is

`gameboy-rust` is a from-scratch Game Boy family emulator in Rust. The Game Boy Advance comes first; other systems (GB, GBC) may follow as separate cores.
It cannot run commercial games yet. See [docs/status.md](docs/status.md) for what works, what is deliberately missing, and what is next.

## Crates

- `crates/gba-core`: the emulator (CPU, memory bus, I/O, timers, DMA, video, BIOS replacement). Keep it free of external dependencies and of window, audio, clock, and file-system code, so it stays portable to WebAssembly.
- `crates/gba-demos`: original ARM test programs and frame runners that drive the core.
- `crates/desktop`: minifb frontend and the `gameboy-rust` binary. Host-specific code belongs here.

[docs/architecture.md](docs/architecture.md) has a file-by-file map.

## Commands

Run through direnv so the Nix toolchain and macOS build flags apply:

```sh
direnv exec . cargo test --locked                                  # everything
direnv exec . cargo test --locked -p gba-core -p gba-demos         # skip the window dependency
direnv exec . cargo clippy --locked --all-targets -- -D warnings
direnv exec . cargo fmt --all
direnv exec . cargo run --locked --release -- --tile-demo          # --help lists all modes
```

Window smoke tests (`--*-smoke-test`) need a logged-in desktop session. Details are in [docs/testing.md](docs/testing.md).

## Where to read

| Working on | Read |
| --- | --- |
| ARM/Thumb instructions, modes, exceptions, timing | [docs/hardware/cpu.md](docs/hardware/cpu.md) |
| Pinned public ARM/Thumb tests and compatibility results | [docs/public-arm-tests.md](docs/public-arm-tests.md), [docs/public-thumb-tests.md](docs/public-thumb-tests.md) |
| Pinned public memory tests and video-byte coverage limits | [docs/public-memory-tests.md](docs/public-memory-tests.md) |
| ROM test suites, assertions, and JSON reports | [docs/rom-tests.md](docs/rom-tests.md) |
| Jev-assisted diagnostic review and `/jev-gb-debug` | [docs/jev-debugging.md](docs/jev-debugging.md) |
| ROM files, window execution, and terminal diagnostics | [docs/hardware/cartridge.md](docs/hardware/cartridge.md) |
| Pinned public BIOS read-protection test and firmware limits | [docs/public-bios-tests.md](docs/public-bios-tests.md) |
| BIOS services (SWI) | [docs/hardware/bios.md](docs/hardware/bios.md) |
| Timers, IE/IF/IME, HALT/STOP | [docs/hardware/timers-irq.md](docs/hardware/timers-irq.md) |
| DMA | [docs/hardware/dma.md](docs/hardware/dma.md) |
| Direct Sound, FIFOs, PSG limits, and mixing | [docs/hardware/audio.md](docs/hardware/audio.md) |
| VCOUNT/DISPSTAT, HBlank/VBlank, row capture | [docs/hardware/display-timing.md](docs/hardware/display-timing.md) |
| Backgrounds, sprites, mosaic, windows, blending, input | [docs/hardware/video.md](docs/hardware/video.md) |
| Demos and key mapping | [docs/demos.md](docs/demos.md) |
| Build environment, minifb workarounds | [docs/development.md](docs/development.md) |

## Where to write

- Behavior notes for a subsystem go in its `docs/hardware/*.md` file. Add a new file there when a new subsystem arrives (audio, cartridge, saves).
- Sources you consulted, including web searches, go in [docs/references.md](docs/references.md), or in a "References" section of the subsystem doc when the link explains one specific behavior.
- Update [docs/status.md](docs/status.md) when a feature lands or a limit goes away.
- Keep `README.md` short: overview, quick start, and the docs index.
- Avoid hard-coded test counts in prose; they go stale with every change.

## Conventions

- Version control is jj (colocated with git). History is a linear stack with one bookmark per change, named `feature/<topic>` or `refactor/<topic>`.
- Commit messages follow Conventional Commits, for example `feat(bios): implement SoftReset restart service`.
- New behavior comes with tests. Core integration tests live in `crates/gba-core/tests/`.
- The core currently reports unsupported hardware behavior as explicit errors rather than guessing. Prefer that to silent approximations, and note the gap in the docs.
- Everything in the repo is original. Do not add Nintendo BIOS code, game ROMs, or game assets.
