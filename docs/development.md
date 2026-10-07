# Development environment

**macOS (Darwin) is the first development and validation target.**
Build and test new features on Apple Silicon macOS before expanding platform support.
Future window, input, and audio libraries must support native macOS.

Install Nix with `nix-command` and `flakes` enabled, plus direnv.
The flake defines shells for Apple Silicon macOS, Intel macOS, ARM64 Linux, and x86-64 Linux.
Only Apple Silicon macOS is verified. Intel macOS and Linux remain untested.

Verified locally on macOS 15.7.3:

- Nix system: `aarch64-darwin`.
- Rust host: `aarch64-apple-darwin`.
- Release executable: native Mach-O `arm64`, without Rosetta.
- All tests pass in debug and release builds.
- All ten native window smoke tests pass. The mosaic test submits 128 frames; the other nine submit 60 frames each.
- The eight CPU-driven smoke tests check CPU state, every output pixel, and presentation at VBlank entry.
- The terminal-only CPU and timer IRQ demos run successfully.
- The bounded terminal ROM runner loads original raw programs and reports state without opening a window.
- The native ROM-window test presents a file-backed original program and reports STOP, CPU, and video failures.
- The headless suite runner passes generated ARM/Thumb/BIOS cases, produces repeatable JSON, and rejects deliberate assertion mismatches.

These results cover the CPU core, memory, Mode 0–5 snapshots and row capture, input mapping, and desktop windows.
They do not verify a complete GBA display controller or hardware audio fidelity.
CPAL 0.16 is a macOS-only dependency of the desktop crate. The existing Nix Darwin shell builds its CoreAudio backend without extra shell packages.
Native tests submit an original tone and original ROM-generated samples to the default output device.
Use `--audio` with a ROM window to opt in. Ordinary tests and muted windows do not require an audio device.
Other host audio platforms are not implemented. Underruns remain possible; device callbacks do not prove listening quality.
Arrow-key movement in the original host display test was confirmed manually on this Mac.
CPU demos and ROM windows have automated input-mapping tests; their physical keyboard behavior still needs manual confirmation.

Enable the direnv hook in your shell if it is not already configured.
For zsh, add this line to `~/.zshrc`:

```sh
eval "$(direnv hook zsh)"
```

For bash, use `eval "$(direnv hook bash)"` in `~/.bashrc` instead.
Restart your shell after changing its configuration.

From this project directory:

```sh
direnv allow
cargo run
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

To run commands without a shell hook:

```sh
direnv allow .
direnv exec . cargo run --locked
direnv exec . cargo test --locked
```

`.envrc` uses the Nix flake to load Rust, Cargo, rustfmt, Clippy, and Python 3.
Python supports the pinned public-test preparation adapter and its standard-library-only tests.
`flake.lock` pins the Nix package source. Keep it in version control.
The first run needs network access to download the development tools and Rust dependencies.

The pinned Nix Darwin toolchain targets macOS 14.0 or newer.
The flake sets Darwin-only `CFLAGS` to correct two minifb 0.28 native-build issues:

- Override its macOS 10.10 deployment flag, which predates the Metal APIs it uses.
- Use `-fcommon` for its shared tentative Objective-C global definition.

Build through direnv or `nix develop` so these settings apply.
The window runs on the main thread, as required by macOS AppKit.
`crates/desktop/src/desktop.rs` also corrects the inverted macOS focus result in minifb 0.28.0.
The dependency is pinned to that exact version. Review this workaround before upgrading minifb.
Intel macOS and Linux desktop builds are not verified.

## Portability boundary

The Rust `gba-core` owns CPU, memory, video, audio generation, input state, and cartridge protocols.
It has no external dependencies or filesystem, window, host-clock, or audio-device code.
The same core can serve another frontend; porting does not require rewriting emulated hardware.
This separation is a design boundary, not a completed Linux or WebAssembly port.

- Linux can reuse the desktop structure and Unix raw-save adapter. Build dependencies and native behavior still need validation.
  Host audio currently returns an explicit unsupported-platform error outside macOS; add and test a Linux backend separately.
- A browser needs WebAssembly bindings, canvas presentation, browser input, Web Audio, and browser scheduling.
  Its frontend must load ROM/image bytes and persist save images through browser storage or explicit import/export.
  It cannot reuse desktop `.sav` filesystem operations or CoreAudio.
- Core save loading/inspection is byte-based. Flash commands never access host files.
  Desktop [save persistence](hardware/saves.md#desktop-persistence) is tested on Darwin arm64 only.

Cross-compilation, performance, lifecycle handling, and save durability need tests on each target.
The core currently uses Rust's standard library; it is dependency-free, not a `no_std` crate.

Without direnv, use:

```sh
nix develop
```

For one command without entering a shell:

```sh
nix develop -c cargo test
```
