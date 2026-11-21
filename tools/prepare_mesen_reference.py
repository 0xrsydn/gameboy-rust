#!/usr/bin/env python3
"""Build the Mesen 2 reference (`mesen-ref`) for differential testing with gba-diff.

This adapter is original. It downloads a pinned source tarball of a single-threaded,
headless trim of Mesen 2, verifies its size and SHA-256, compiles the GBA core into a static
library, and links the original shim in tools/mesen-ref/ against it. Mesen 2 (GPL-3.0-only)
is only built in the requested local directory; none of its source is copied into this
repository, and the linked binary, a GPL-3.0-only combined work, stays there too.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile

from prepare_mgba_reference import destination, extract, fetch, run, verify

LOCK = Path(__file__).with_name("mesen-reference.lock.json")
SHIM_DIR = Path(__file__).with_name("mesen-ref")
SHIM_SOURCES = ["mesen_ref.cpp", "debugger_stubs.cpp"]

# The fork keeps only the GBA core. Its Debugger directory still holds the Lua bindings and
# the multi-console disassembler and expression evaluator, which reference deleted cores;
# mesen-ref never creates a debugger, and debugger_stubs.cpp stands in for the latter three.
CORE_DIRS = ["Core/GBA", "Core/Shared", "Utilities", "SevenZip"]
DEBUGGER_DIR = "Core/Debugger"
DEBUGGER_EXCLUDED = {
    "LuaApi.cpp", "LuaScriptingContext.cpp", "LuaCallHelper.cpp", "ScriptManager.cpp",
    "ScriptHost.cpp", "Disassembler.cpp", "DisassemblyInfo.cpp", "ExpressionEvaluator.cpp",
}

# MESEN_HEADLESS selects the fork's single-threaded, GBA-only paths. Mesen type-puns, so
# strict aliasing stays off; the fork removed every dynamic_cast and builds without RTTI.
DEFINES = ["-DMESEN_HEADLESS"]
OPTIMIZE = ["-O2", "-fno-strict-aliasing"]
CXX_FLAGS = ["-std=c++17", "-fno-rtti"]


def includes(source, flag):
    # Utilities is on the path because Mesen includes some of its headers with angle brackets.
    return [f"{flag}{source / part}" for part in ("Core", "", "Utilities")]


def sources(source):
    found = []
    for directory in CORE_DIRS:
        found += [path for path in sorted((source / directory).rglob("*"))
                  if path.suffix in (".c", ".cpp") and path.is_file()]
    found += [path for path in sorted((source / DEBUGGER_DIR).rglob("*.cpp"))
              if path.name not in DEBUGGER_EXCLUDED and path.is_file()]
    return found


def compile_core(source, objects, log):
    """Compile every core source in parallel; stop with a log of the first failures."""
    objects.mkdir()
    units = sources(source)
    if not units:
        raise ValueError("unexpected tarball layout: no GBA core sources")

    def compile_one(item):
        index, path = item
        compiler = "cc" if path.suffix == ".c" else "c++"
        flags = [] if path.suffix == ".c" else CXX_FLAGS
        # Third-party code: its warnings are not this repository's to fix.
        command = [compiler, *DEFINES, *includes(source, "-I"), *OPTIMIZE, *flags, "-w",
                   "-c", str(path), "-o", str(objects / f"{index:04}.o")]
        result = subprocess.run(command, capture_output=True, text=True)
        return path, result

    with ThreadPoolExecutor(max_workers=os.cpu_count() or 2) as pool:
        results = list(pool.map(compile_one, enumerate(units)))
    failures = [(path, result) for path, result in results if result.returncode != 0]
    with open(log, "w", encoding="utf-8") as stream:
        stream.write(f"{len(units)} sources, {len(failures)} failed\n")
        for path, result in failures:
            stream.write(f"--- {path.relative_to(source)}\n{result.stdout}{result.stderr}\n")
    if failures:
        raise RuntimeError(f"{len(failures)} Mesen sources failed to compile; see {log}")
    return sorted(objects.glob("*.o"))


def link_flags():
    return [] if sys.platform == "darwin" else ["-lpthread"]


def prepare(output, tarball=None, download=fetch, build=True):
    output = destination(output)
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    if tarball is None:
        data = download(lock["url"])
    else:
        data = Path(tarball).read_bytes()
    verify(data, lock)
    if build:
        for tool in ("c++", "cc", "ar"):
            if shutil.which(tool) is None:
                raise RuntimeError(f"{tool} is required; build through `direnv exec .`, whose dev shell has it")
    output.mkdir()
    archive = output / "mesen.tar.gz"
    archive.write_bytes(data)
    source = extract(archive, output / "src")
    if not build:
        return output
    objects = compile_core(source, output / "build", output / "core-build.log")
    library = output / "libmesen-gba.a"
    run(["ar", "rcs", str(library), *map(str, objects)], output / "archive.log")
    shim = [str(SHIM_DIR / name) for name in SHIM_SOURCES]
    # Mesen's headers are system includes here, so -Wall reports only the shim's own code.
    run(["c++", *DEFINES, *includes(source, "-isystem"), *OPTIMIZE, *CXX_FLAGS, "-Wall",
         *shim, str(library), *link_flags(), "-o", str(output / "mesen-ref")],
        output / "shim-build.log")
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
    print(output / "mesen-ref")
    return 0


if __name__ == "__main__":
    sys.exit(main())
