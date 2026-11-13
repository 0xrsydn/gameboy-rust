#!/usr/bin/env python3
"""Build the mGBA reference (`mgba-ref`) for differential testing with gba-diff.

This adapter is original. It downloads a pinned mGBA release source tarball,
verifies its SHA-256, builds a minimal static libmgba, and links the original
shim in tools/mgba-ref/mgba_ref.c against it. mGBA (MPL-2.0) is only built in the
requested local directory; none of its source is copied into this repository.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
LOCK = Path(__file__).with_name("mgba-reference.lock.json")
SHIM = Path(__file__).with_name("mgba-ref") / "mgba_ref.c"
MAX_TARBALL_BYTES = 64 * 1024 * 1024

# Headless core only: no frontends, renderers, scripting, or optional libraries.
CMAKE_OPTIONS = [
    "-DCMAKE_BUILD_TYPE=Release",
    # mGBA 0.10 declares an old cmake_minimum_required that CMake 4 rejects.
    "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
    "-DBUILD_STATIC=ON", "-DBUILD_SHARED=OFF",
    "-DBUILD_QT=OFF", "-DBUILD_SDL=OFF", "-DBUILD_LIBRETRO=OFF",
    "-DBUILD_GL=OFF", "-DBUILD_GLES2=OFF", "-DBUILD_GLES3=OFF",
    "-DBUILD_ROM_TEST=OFF", "-DBUILD_SUITE=OFF", "-DBUILD_CINEMA=OFF",
    # mgba-perf is kept for later benchmarking.
    "-DBUILD_PERF=ON",
    "-DUSE_FFMPEG=OFF", "-DUSE_ZLIB=OFF", "-DUSE_MINIZIP=OFF", "-DUSE_PNG=OFF",
    "-DUSE_LIBZIP=OFF", "-DUSE_SQLITE3=OFF", "-DUSE_ELF=OFF", "-DUSE_LUA=OFF",
    "-DUSE_LZMA=OFF", "-DUSE_DISCORD_RPC=OFF", "-DUSE_EPOXY=OFF",
    "-DUSE_EDITLINE=OFF", "-DUSE_GDB_STUB=OFF", "-DUSE_DEBUGGERS=OFF",
    "-DENABLE_SCRIPTING=OFF",
]


def fetch(url):
    request = Request(url, headers={"User-Agent": "gameboy-rust-reference-adapter"})
    with urlopen(request, timeout=60) as response:
        return response.read(MAX_TARBALL_BYTES + 1)


def verify(data, lock):
    if len(data) > MAX_TARBALL_BYTES or len(data) != lock["bytes"]:
        raise ValueError("tarball size mismatch")
    if hashlib.sha256(data).hexdigest() != lock["sha256"]:
        raise ValueError("tarball SHA-256 mismatch")


def destination(path):
    path = Path(path).resolve()
    if path.is_relative_to(ROOT) and not path.is_relative_to(ROOT / "roms"):
        raise ValueError("output must be outside the repository or inside ignored roms/")
    if path.exists():
        raise ValueError(f"output directory already exists: {path}")
    if not path.parent.is_dir():
        raise ValueError("output parent directory must already exist")
    return path


def extract(tarball, target):
    with tarfile.open(tarball, "r:gz") as archive:
        # The "data" filter rejects absolute paths, links outside the tree, and devices.
        archive.extractall(target, filter="data")
    roots = [entry for entry in target.iterdir() if entry.is_dir()]
    if len(roots) != 1:
        raise ValueError("unexpected tarball layout")
    return roots[0]


def run(command, log):
    with open(log, "w", encoding="utf-8") as stream:
        result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode != 0:
        raise RuntimeError(f"command failed ({result.returncode}); see {log}")


def link_flags():
    if platform.system() == "Darwin":
        return ["-lpthread", "-framework", "Foundation"]
    return ["-lpthread", "-lm"]


def prepare(output, tarball=None, download=fetch, build=True):
    output = destination(output)
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    if tarball is None:
        data = download(lock["url"])
    else:
        data = Path(tarball).read_bytes()
    verify(data, lock)
    if build:
        for tool in ("cmake", "cc"):
            if shutil.which(tool) is None:
                raise RuntimeError(f"{tool} is required; try `nix shell nixpkgs#cmake`")
    output.mkdir()
    archive = output / "mgba.tar.gz"
    archive.write_bytes(data)
    source = extract(archive, output / "src")
    if not build:
        return output
    objects = output / "build"
    flags = os.environ.get("CFLAGS", "")
    # Newer SDKs flag APIs above mGBA's old deployment target; they are unused here.
    flags += " -Wno-error=unguarded-availability -Wno-error=unguarded-availability-new"
    run(["cmake", "-S", str(source), "-B", str(objects), f"-DCMAKE_C_FLAGS={flags}"]
        + CMAKE_OPTIONS, output / "cmake-configure.log")
    run(["cmake", "--build", str(objects), "-j", str(os.cpu_count() or 2)],
        output / "cmake-build.log")
    run(["cc", "-O2", "-std=gnu11", "-Wall", f"-I{source / 'include'}",
         f"-I{objects / 'include'}", "-DBUILD_STATIC", str(SHIM),
         str(objects / "libmgba.a"), *link_flags(), "-o", str(output / "mgba-ref")],
        output / "shim-build.log")
    perf = objects / "mgba-perf"
    if perf.exists():
        shutil.copy2(perf, output / "mgba-perf")
    (output / "source-lock.json").write_text(json.dumps(lock, indent=2) + "\n",
                                             encoding="utf-8")
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", help="new directory outside the repository or in roms/")
    parser.add_argument("--tarball", help="use a local copy of the pinned source tarball")
    args = parser.parse_args()
    try:
        output = prepare(args.output, tarball=args.tarball)
    except (OSError, ValueError, RuntimeError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(output / "mgba-ref")
    return 0


if __name__ == "__main__":
    sys.exit(main())
