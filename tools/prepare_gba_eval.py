#!/usr/bin/env python3
"""Fetch the GBA Eval benchmark's BIOS stub and test corpus for local evaluation.

This adapter is original. It downloads the pinned archive of github.com/mechanize-work/gba-eval,
verifies its size and SHA-256, and extracts only what local scoring needs: the BIOS stub
(spec/, MIT), the ABI text, and the corpus (testcases.json, replays, and ROMs, each under its
own license; corpus/LICENSE there lists them). Nothing is copied into this repository; the
destination must be outside it or inside the ignored roms/. Every ROM is checked against the
SHA-256 that testcases.json records.

Afterwards build the benchmark library with the stub embedded:

    GBA_EVAL_BIOS=DEST/gba_bios_stub.bin cargo build --release --lib \\
        --target wasm32-unknown-unknown -p gba-eval
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tarfile
from urllib.request import Request, urlopen

from prepare_mgba_reference import destination

LOCK = Path(__file__).with_name("gba-eval.lock.json")
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
BIOS = "spec/gba_bios_stub.bin"
# Archive paths kept, relative to its top directory.
KEEP_FILES = {BIOS, "spec/ABI.md", "spec/LICENSE", "LICENSE", "LICENSE-MIT", "NOTICE",
              "corpus/testcases.json", "corpus/grader.yaml", "corpus/README.md",
              "corpus/LICENSE"}
KEEP_DIRS = ("corpus/replays/", "corpus/roms/")


def fetch(url):
    request = Request(url, headers={"User-Agent": "gameboy-rust-eval-adapter"})
    with urlopen(request, timeout=120) as response:
        return response.read(MAX_ARCHIVE_BYTES + 1)


def verify(data, lock):
    if len(data) > MAX_ARCHIVE_BYTES or len(data) != lock["bytes"]:
        raise ValueError("archive size mismatch")
    if hashlib.sha256(data).hexdigest() != lock["sha256"]:
        raise ValueError("archive SHA-256 mismatch")


def kept(relative):
    return relative in KEEP_FILES or relative.startswith(KEEP_DIRS)


def extract(archive, output):
    """Extract the kept regular files, flattening the archive's top directory."""
    with tarfile.open(archive, "r:gz") as tar:
        members = []
        for member in tar.getmembers():
            parts = member.name.split("/", 1)
            if len(parts) != 2 or not member.isfile() or not kept(parts[1]):
                continue
            member.name = parts[1]
            members.append(member)
        # The "data" filter rejects absolute paths, links outside the tree, and devices.
        tar.extractall(output, members=members, filter="data")


def check_corpus(output, lock):
    stub = (output / BIOS).read_bytes()
    if len(stub) != 16 * 1024 or hashlib.sha256(stub).hexdigest() != lock["bios_sha256"]:
        raise ValueError("BIOS stub size or SHA-256 mismatch")
    roms = {hashlib.sha256(path.read_bytes()).hexdigest(): path
            for path in (output / "corpus" / "roms").rglob("*.gba")}
    cases = json.loads((output / "corpus" / "testcases.json").read_text(encoding="utf-8"))
    cases = cases["testcases"]
    missing = [case["id"] for case in cases if case["rom_sha256"] not in roms]
    return len(cases), missing


def prepare(output, archive=None, download=fetch):
    output = destination(output)
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    data = download(lock["url"]) if archive is None else Path(archive).read_bytes()
    verify(data, lock)
    output.mkdir()
    tarball = output / "gba-eval.tar.gz"
    tarball.write_bytes(data)
    extract(tarball, output)
    tarball.unlink()
    # The stub sits at the top level too, for GBA_EVAL_BIOS.
    (output / "gba_bios_stub.bin").write_bytes((output / BIOS).read_bytes())
    count, missing = check_corpus(output, lock)
    (output / "source-lock.json").write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
    return output, count, missing


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", help="new directory outside the repository or in roms/")
    parser.add_argument("--archive", help="use a local copy of the pinned archive")
    args = parser.parse_args()
    try:
        output, count, missing = prepare(args.output, archive=args.archive)
    except (OSError, ValueError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"{output}: BIOS stub verified; {count} testcases", end="")
    print(f", {len(missing)} without their ROM: {', '.join(missing)}" if missing else "")
    return 0


if __name__ == "__main__":
    sys.exit(main())
