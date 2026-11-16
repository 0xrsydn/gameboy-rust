#!/usr/bin/env python3
"""Summarize a samply profile of a `--profile profiling` build, inlined functions included.

usage: python3 tools/profile/report.py PROFILE.json.gz BINARY [N] [--callers TEXT]

PROFILE comes from `samply record --save-only --unstable-presymbolicate -o PROFILE -- BINARY ...`.
Each sampled address in BINARY is expanded with `atos -i` into its chain of inlined functions,
so time inside the release build's inlined hot path is attributed to the function and source
line it came from. Prints the top N (default 25) of three tables:

- self time by source line: where the program counter was;
- self time by function: the innermost function at that line;
- total time by function: every function on the stack, inlined or not, counted once per sample.

With --callers TEXT, also ranks the call paths (three callers deep) into the innermost function whose
name contains TEXT, by the time spent under them: which caller owns a hot helper such as `clone`.

macOS only (atos). See docs/development.md.
"""

import gzip
import json
import os
import re
import subprocess
import sys
from collections import Counter

ATOS_LINE = re.compile(r"^(?P<name>.*?) \(in [^)]*\)(?: \((?P<where>[^)]*)\))?(?: \+ \d+)?$")


def load(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt", encoding="utf-8") as file:
        profile = json.load(file)
    base = path[: -len(".gz")] if path.endswith(".gz") else path
    symbols_path = base[: -len(".json")] + ".json.syms.json" if base.endswith(".json") else None
    symbols = None
    if symbols_path and os.path.exists(symbols_path):
        with open(symbols_path, encoding="utf-8") as file:
            symbols = json.load(file)
    return profile, symbols


def symbol_lookup(symbols, profile):
    """Return lookup(lib_index, rva) -> demangled symbol name, from the presymbolicated sidecar."""
    tables = {}
    if symbols:
        strings = symbols["string_table"]
        by_id = {entry["code_id"]: entry for entry in symbols["data"]}
        for index, lib in enumerate(profile["libs"]):
            entry = by_id.get(lib.get("codeId"))
            if entry:
                rows = sorted((row["rva"], row["size"], strings[row["symbol"]]) for row in entry["symbol_table"])
                tables[index] = rows

    def lookup(lib, rva):
        name = profile["libs"][lib]["name"]
        for start, size, symbol in reversed(tables.get(lib, [])):
            if start <= rva:
                return symbol if rva < start + size else f"{name}+{rva:#x}"
        return f"{name}+{rva:#x}"

    return lookup


def text_vmaddr(binary):
    output = subprocess.run(["otool", "-l", binary], capture_output=True, text=True, check=True).stdout
    match = re.search(r"segname __TEXT\s+vmaddr (0x[0-9a-f]+)", output)
    return int(match.group(1), 16) if match else 0x100000000


def inline_chains(binary, rvas):
    """Map each rva to its inline chain [(name, where)], outermost first."""
    vmaddr = text_vmaddr(binary)
    chains = {}
    rvas = sorted(rvas)
    for start in range(0, len(rvas), 400):
        batch = rvas[start : start + 400]
        command = ["atos", "-i", "-o", binary, "-l", hex(vmaddr)] + [hex(vmaddr + rva) for rva in batch]
        output = subprocess.run(command, capture_output=True, text=True, check=True).stdout
        blocks = output.rstrip("\n").split("\n\n")
        if len(blocks) != len(batch):
            sys.exit(f"atos returned {len(blocks)} entries for {len(batch)} addresses")
        for rva, block in zip(batch, blocks):
            chain = []
            for line in block.splitlines():
                match = ATOS_LINE.match(line.strip())
                chain.append((match["name"], match["where"] or "?") if match else (line.strip(), "?"))
            chains[rva] = list(reversed(chain))
    return chains


def main():
    argv = sys.argv[1:]
    callers = None
    if "--callers" in argv:
        index = argv.index("--callers")
        callers = argv[index + 1]
        del argv[index : index + 2]
    if len(argv) not in (2, 3):
        sys.exit(__doc__.strip().splitlines()[2])
    if sys.platform != "darwin":
        sys.exit("report.py symbolizes with atos and runs on macOS only")
    profile_path, binary = argv[0], argv[1]
    limit = int(argv[2]) if len(argv) == 3 else 25
    profile, symbols = load(profile_path)
    lookup = symbol_lookup(symbols, profile)
    thread = max(profile["threads"], key=lambda thread: thread["samples"]["length"])
    frames, stacks = thread["frameTable"], thread["stackTable"]
    funcs, resources = thread["funcTable"], thread["resourceTable"]
    binary_name = os.path.basename(binary)

    def frame_lib(frame):
        resource = funcs["resource"][frames["func"][frame]]
        return resources["lib"][resource] if resource is not None and resource >= 0 else None

    weights = thread["samples"].get("weight") or [1] * thread["samples"]["length"]
    samples = []  # (weight, [(lib, rva, is_leaf)] leaf first)
    wanted = set()
    for stack, weight in zip(thread["samples"]["stack"], weights):
        walk = []
        while stack is not None:
            frame = stacks["frame"][stack]
            lib = frame_lib(frame)
            rva = frames["address"][frame]
            if lib is not None and rva is not None and rva >= 0:
                walk.append((lib, rva))
                if profile["libs"][lib]["name"] == binary_name:
                    wanted.add(rva)
            stack = stacks["prefix"][stack]
        if walk:
            samples.append((weight or 1, walk))

    chains = inline_chains(binary, wanted)
    total = sum(weight for weight, _ in samples)
    self_line, self_func, total_func = Counter(), Counter(), Counter()
    call_sites = Counter()

    def expand(lib, rva):
        if profile["libs"][lib]["name"] == binary_name:
            chain = chains[rva]
            outer = lookup(lib, rva)
            # atos names inlined functions without their path; add the file to tell them apart.
            return [
                (outer if index == 0 else f"{name} ({where.rsplit(':', 1)[0]})", where)
                for index, (name, where) in enumerate(chain)
            ]
        return [(lookup(lib, rva), profile["libs"][lib]["name"])]

    for weight, walk in samples:
        leaf = expand(*walk[0])
        name, where = leaf[-1]
        self_line[f"{where}  {name}"] += weight
        self_func[name] += weight
        seen = set()
        for lib, rva in walk:
            for name, _ in expand(lib, rva):
                if name not in seen:
                    seen.add(name)
                    total_func[name] += weight
        if callers is not None:
            # Innermost first; each frame's location is where it calls the frame inside it.
            inner_first = [frame for lib, rva in walk for frame in reversed(expand(lib, rva))]
            for depth, (name, _) in enumerate(inner_first):
                if callers in name:
                    # Three callers, nearest first: the nearest is often another std helper.
                    outer = inner_first[depth + 1 : depth + 4]
                    call_sites[" <- ".join(f"{where} {name}" for name, where in outer)] += weight
                    break

    print(f"{total} samples from thread {thread['name']}")
    for title, table in (
        ("self time by source line", self_line),
        ("self time by function", self_func),
        ("total time by function", total_func),
    ):
        print(f"\n{title}:")
        for key, count in table.most_common(limit):
            print(f"{count:8} {100 * count / max(total, 1):5.1f}%  {key}")
    if callers is not None:
        print(f"\ncall sites of the innermost function matching {callers!r} (total time):")
        for key, count in call_sites.most_common(limit):
            print(f"{count:8} {100 * count / max(total, 1):5.1f}%  {key}")


if __name__ == "__main__":
    main()
