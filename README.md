# gameboy-rust

Game Boy family emulation in Rust, written from scratch.
The Game Boy Advance (GBA) is the first and currently only system.

**This cannot run Pokémon Emerald or other games yet.**
It is an emulator foundation: an ARM7TDMI interpreter, memory bus, timers, DMA, Mode 0–5 video, and an original BIOS replacement.
The included demos run original ARM test programs through the emulated hardware. ROM loading, audio, and saves are not implemented.

## Layout

| Crate | Purpose |
| --- | --- |
| `crates/gba-core` | Platform-independent GBA emulator. No external Rust dependencies. |
| `crates/gba-demos` | Original test programs that drive the core. |
| `crates/desktop` | Native window frontend on minifb. Builds the `gameboy-rust` executable. |

## Quick start

macOS on Apple Silicon is the verified platform. Install Nix (with flakes) and direnv, then:

```sh
direnv allow .
direnv exec . cargo run --locked --release -- --tile-demo
direnv exec . cargo test --locked
```

Arrows scroll, Q rotates, W zooms, Z flips, X changes priority, Enter resets, and Escape exits.
Run `direnv exec . cargo run --locked -- --help` to list every demo and smoke test.

## Documentation

| Document | Contents |
| --- | --- |
| [docs/status.md](docs/status.md) | What works today, deliberate limits, and next steps |
| [docs/demos.md](docs/demos.md) | Every demo, its command, and its controls |
| [docs/development.md](docs/development.md) | Nix, direnv, and macOS build notes |
| [docs/testing.md](docs/testing.md) | Validation commands and what the test suites cover |
| [docs/architecture.md](docs/architecture.md) | Crate boundaries and a file-by-file map |
| [docs/hardware/cpu.md](docs/hardware/cpu.md) | ARM/Thumb execution rules, modes, exceptions, and timing |
| [docs/hardware/bios.md](docs/hardware/bios.md) | The original BIOS replacement and each supported service |
| [docs/hardware/timers-irq.md](docs/hardware/timers-irq.md) | Timers, interrupt registers, HALT/STOP, and power control |
| [docs/hardware/dma.md](docs/hardware/dma.md) | DMA channels, triggers, and timing |
| [docs/hardware/display-timing.md](docs/hardware/display-timing.md) | Display clock, status, IRQs, and scanline capture |
| [docs/hardware/video.md](docs/hardware/video.md) | Backgrounds, sprites, mosaic, windows, color effects, and input |
| [docs/references.md](docs/references.md) | External hardware references and emulator sources consulted |

Coding agents should start with [AGENTS.md](AGENTS.md).

## Legal

All demo content and the BIOS replacement are original. No Nintendo code or assets are included.
Only use game ROMs that you may lawfully use. Do not commit game ROMs, BIOS files, or game assets.
