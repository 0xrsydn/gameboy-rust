#!/usr/bin/env python3
"""Prepare a pinned public ARM, Thumb, or memory test outside tracked paths.

This adapter is original. Upstream sources, license, and ROM bytes are downloaded
only to the requested local directory. They are not embedded in this repository.
"""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
LOCKS = {suite: Path(__file__).with_name(f"gba-tests-{suite}.lock.json")
         for suite in ("arm", "thumb", "memory")}
MAX_FILE_BYTES = 1024 * 1024


def fetch(url):
    request = Request(url, headers={"User-Agent": "gameboy-rust-public-test-adapter"})
    with urlopen(request, timeout=30) as response:
        return response.read(MAX_FILE_BYTES + 1)


def verify(name, data, expected):
    if len(data) > MAX_FILE_BYTES or len(data) != expected["bytes"]:
        raise ValueError(f"size mismatch for {name}")
    if hashlib.sha256(data).hexdigest() != expected["sha256"]:
        raise ValueError(f"SHA-256 mismatch for {name}")


def destination(path):
    path = Path(path).resolve()
    # Public ROM headers can contain third-party assets. Keep them untracked.
    if path.is_relative_to(ROOT) and not path.is_relative_to(ROOT / "roms"):
        raise ValueError("output must be outside the repository or inside ignored roms/")
    if path.exists():
        raise ValueError(f"output directory already exists: {path}")
    if not path.parent.is_dir():
        raise ValueError("output parent directory must already exist")
    return path


def prepare(output, source=None, download=fetch, suite="arm"):
    if suite not in LOCKS:
        raise ValueError(f"unsupported public suite: {suite}")
    output = destination(output)
    lock = json.loads(LOCKS[suite].read_text(encoding="utf-8"))
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

    # The source-defined eval label starts with m_vsync's register save. All
    # failure exits set the result register (ARM/memory r12, Thumb r7).
    # Thumb tests switch back to ARM before eval. Success leaves the result zero.
    # Hash pinning is authoritative; these checks catch adapter-offset mistakes.
    pc = int(lock["completion_pc"], 16)
    offset = pc - 0x08000000
    rom = files[lock["rom"]]
    if offset < 0 or offset % 4 or offset + 4 > len(rom) or struct.unpack_from("<I", rom, offset)[0] != 0xE92D0003:
        raise ValueError("pinned evaluation checkpoint does not match the ROM")
    if suite == "thumb" and (offset < 4 or struct.unpack_from("<HH", rom, offset - 4) != (0xA000, 0x4700)):
        raise ValueError("pinned Thumb-to-ARM checkpoint bridge does not match the ROM")
    manifest = {
        "version": 1,
        "cases": [{
            "name": f"jsmolka-{suite}",
            "rom": lock["rom"],
            "step_limit": 1_000_000,
            "completion": {"pc": lock["completion_pc"], "instruction_set": "arm"},
            "checks": [{"kind": "register", "index": lock["result_register"], "equals": 0}],
        }],
    }
    # Validate every file before creating output. Existing destinations are never
    # reused, and file creation is exclusive. Disk errors can leave partial output.
    output.mkdir()
    for name, data in files.items():
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(data)
    for name, value in [("suite.json", manifest), ("source-lock.json", lock)]:
        with (output / name).open("x", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
    return output / "suite.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory outside the repository or under roms/")
    parser.add_argument("--suite", choices=tuple(LOCKS), default="arm", help="public suite to prepare (default: arm)")
    parser.add_argument("--source-dir", type=Path, help="offline upstream tree; every selected file must match the lock")
    args = parser.parse_args()
    try:
        manifest = prepare(args.output, args.source_dir, suite=args.suite)
    except (OSError, ValueError) as error:
        print(f"cannot prepare public {args.suite} test: {error}", file=sys.stderr)
        return 1
    print(f"Prepared pinned public {args.suite} suite: {manifest}")
    print("Run with gameboy-rust --test-suite PATH. Preparation does not mean the tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
