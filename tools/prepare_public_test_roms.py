#!/usr/bin/env python3
"""Fetch pinned public GBA hardware-test ROMs for local surveys and comparisons.

This adapter is original. For each suite in public-test-roms.lock.json it downloads the
pinned GitHub archive, verifies its size and SHA-256, and extracts only the prebuilt `.gba`
files, plus the suite's license, into DEST/<suite>/. Nothing is copied into this repository;
DEST must be outside it or inside the ignored roms/. Use `--archives DIR` for offline copies
named like `<owner>_<repo>.tar.gz`.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tarfile
from urllib.request import Request, urlopen

from prepare_mgba_reference import destination

LOCK = Path(__file__).with_name("public-test-roms.lock.json")
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024


def fetch(url):
    request = Request(url, headers={"User-Agent": "gameboy-rust-test-rom-adapter"})
    with urlopen(request, timeout=120) as response:
        return response.read(MAX_ARCHIVE_BYTES + 1)


def verify(data, suite):
    if len(data) > MAX_ARCHIVE_BYTES or len(data) != suite["bytes"]:
        raise ValueError(f"{suite['name']}: archive size mismatch")
    if hashlib.sha256(data).hexdigest() != suite["sha256"]:
        raise ValueError(f"{suite['name']}: archive SHA-256 mismatch")


def kept(relative):
    name = relative.rsplit("/", 1)[-1].lower()
    return relative.lower().endswith(".gba") or name.startswith("license")


def extract(archive, target):
    """Extract the ROMs and licenses, flattening the archive's top directory."""
    count = 0
    with tarfile.open(archive, "r:gz") as tar:
        members = []
        for member in tar.getmembers():
            parts = member.name.split("/", 1)
            if len(parts) != 2 or not member.isfile() or not kept(parts[1]):
                continue
            member.name = parts[1]
            members.append(member)
            count += member.name.lower().endswith(".gba")
        # The "data" filter rejects absolute paths, links outside the tree, and devices.
        tar.extractall(target, members=members, filter="data")
    return count


def prepare(output, archives=None, download=fetch):
    output = destination(output)
    suites = json.loads(LOCK.read_text(encoding="utf-8"))["suites"]
    blobs = []
    for suite in suites:
        if archives is None:
            data = download(suite["url"])
        else:
            data = (Path(archives) / (suite["project"].replace("/", "_") + ".tar.gz")).read_bytes()
        verify(data, suite)
        blobs.append((suite, data))
    output.mkdir()
    counts = {}
    for suite, data in blobs:
        archive = output / f"{suite['name']}.tar.gz"
        archive.write_bytes(data)
        counts[suite["name"]] = extract(archive, output / suite["name"])
        archive.unlink()
    (output / "source-lock.json").write_text(LOCK.read_text(encoding="utf-8"), encoding="utf-8")
    return output, counts


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", help="new directory outside the repository or in roms/")
    parser.add_argument("--archives", help="directory of local copies of the pinned archives")
    args = parser.parse_args()
    try:
        output, counts = prepare(args.output, archives=args.archives)
    except (OSError, ValueError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"{output}: " + ", ".join(f"{name} {count} ROMs" for name, count in counts.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
