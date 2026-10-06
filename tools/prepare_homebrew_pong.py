#!/usr/bin/env python3
"""Prepare hash-pinned public Pong gameplay checks outside tracked paths.

Only original adapter logic is included here. Downloaded source, assets, ROM,
and license stay in the requested local directory. A successful preparation
is not a compatibility verdict. Run the generated suite separately.
"""

import argparse
import json
from pathlib import Path
import sys

from prepare_gba_tests import MAX_FILE_BYTES, destination, fetch, verify

LOCK = Path(__file__).with_name("homebrew-pong.lock.json")


def manifest(rom):
    """Sparse assertions derived from the pinned source, not emulator screenshots."""
    white, blue, green = 0xffffff, 0x0000ff, 0x00ff00

    def pixel(x, y, color):
        return {"kind": "pixel", "x": x, "y": y, "equals": color}

    def inputs(*pairs):
        return [{"vblank": frame, "buttons": buttons} for frame, buttons in pairs]

    def case(name, end, events, checks):
        return {"name": f"pong-{name}", "rom": rom, "step_limit": 3_000_000,
                "completion": {"vblanks": end}, "inputs": events, "checks": checks}

    start = inputs((30, 8), (32, 0))  # Start, release
    move = start + inputs((160, 64), (170, 0))  # Up, release after initial pause
    menu = inputs((30, 128), (32, 0), (40, 1), (42, 0))  # Down, A
    return {"version": 2, "cases": [
        case("menu", 30, [], [pixel(81, 64, white), pixel(74, 88, white), pixel(74, 100, 0)]),
        case("settings", 50, menu, [pixel(81, 64, 0), pixel(84, 64, white),
                                    pixel(74, 88, 0), pixel(74, 100, white)]),
        case("back", 70, menu + inputs((60, 8), (62, 0)),
             [pixel(81, 64, white), pixel(74, 100, white)]),
        case("start", 40, start, [pixel(2, 70, blue), pixel(232, 70, blue),
                                 pixel(118, 77, green), pixel(120, 2, white)]),
        case("ball-motion", 160, start, [pixel(118, 77, 0), pixel(135, 95, green)]),
        case("up", 175, move, [pixel(2, 50, blue), pixel(2, 90, 0)]),
        case("release", 185, move, [pixel(2, 50, blue), pixel(2, 90, 0)]),
        case("down", 205, move + inputs((190, 128), (200, 0)),
             [pixel(2, 50, 0), pixel(2, 70, blue)]),
    ]}


def prepare(output, source=None, download=fetch):
    output = destination(output)
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    base = f"https://raw.githubusercontent.com/{lock['repository']}/{lock['revision']}/"
    files = {}
    for name, expected in lock["files"].items():
        if source is None:
            data = download(base + name)
        else:
            path = Path(source) / name
            if not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
                raise ValueError(f"source must be a bounded regular file: {name}")
            with path.open("rb") as stream:
                data = stream.read(MAX_FILE_BYTES + 1)
        verify(name, data, expected)
        files[name] = data
    # Verify the full pinned set before creating output. Never reuse an existing
    # directory or overwrite files. Disk errors can leave partial output.
    output.mkdir()
    for name, data in files.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as stream:
            stream.write(data)
    for name, value in [("suite.json", manifest(lock["rom"])), ("source-lock.json", lock)]:
        with (output / name).open("x", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
    return output / "suite.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory outside the repository or under roms/")
    parser.add_argument("--source-dir", type=Path, help="offline upstream files matching every pinned hash")
    args = parser.parse_args()
    try:
        path = prepare(args.output, args.source_dir)
    except (OSError, ValueError) as error:
        print(f"cannot prepare homebrew Pong: {error}", file=sys.stderr)
        return 1
    print(f"Prepared pinned Pong suite: {path}")
    print("Run with gameboy-rust --test-suite PATH. Preparation does not mean the tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
