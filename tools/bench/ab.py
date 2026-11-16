#!/usr/bin/env python3
"""Compare the emulation speed of two builds with interleaved, paired runs.

usage: python3 tools/bench/ab.py A B [--runs N] [-- REPLAY_HASH_ARGS...]

A and B are replay_hash binaries or jj revisions (built and cached by tools/bench/build-rev.sh).
Each round runs A then B, alternating to B then A on the next round, so drift in host speed
falls on both. The default workload is 10,000 frames of FireRed gameplay with --capture and
--idle-skip, which does the window's work. Prints each run, both medians, and B/A as the median
and range of the per-round ratios. See docs/development.md.
"""

import os
import re
import statistics
import subprocess
import sys

DEFAULT_ARGS = [
    "--rom", "roms/pokemon-firered.gba", "--save-type", "flash128", "--frames", "10000",
    "--inputs", "tools/replays/firered-mtmoon.inputs", "--capture", "--idle-skip",
]
FPS = re.compile(r"([0-9.]+) fps")


def resolve(target):
    if os.path.isfile(target) and os.access(target, os.X_OK):
        return target, target
    built = subprocess.run(
        ["tools/bench/build-rev.sh", target], capture_output=True, text=True
    )
    if built.returncode != 0:
        sys.exit(f"{target}: build failed\n{built.stderr}")
    path = built.stdout.strip().splitlines()[-1]
    return path, f"{target} ({os.path.basename(path).removeprefix('replay_hash-')})"


def measure(binary, args):
    output = subprocess.run([binary, *args, "--bench"], capture_output=True, text=True)
    match = FPS.search(output.stdout)
    if output.returncode != 0 or not match:
        sys.exit(f"{binary} failed:\n{output.stdout}{output.stderr}")
    return float(match.group(1))


def main():
    argv = sys.argv[1:]
    args = DEFAULT_ARGS
    if "--" in argv:
        split = argv.index("--")
        argv, args = argv[:split], argv[split + 1 :]
    runs = 4
    if "--runs" in argv:
        index = argv.index("--runs")
        runs = int(argv[index + 1])
        del argv[index : index + 2]
    if len(argv) != 2 or runs < 1:
        sys.exit(__doc__.strip().splitlines()[2])

    (a, a_name), (b, b_name) = resolve(argv[0]), resolve(argv[1])
    load = os.getloadavg()[0]
    if load > 2:
        print(f"warning: load average {load:.1f}; other work on the host skews the results")
    print(f"A = {a_name}\nB = {b_name}")
    fps = {"A": [], "B": []}
    for round_index in range(runs):
        order = [("A", a), ("B", b)] if round_index % 2 == 0 else [("B", b), ("A", a)]
        for name, binary in order:
            fps[name].append(measure(binary, args))
        print(f"round {round_index + 1}: A {fps['A'][-1]:7.1f} fps   B {fps['B'][-1]:7.1f} fps")

    ratios = [fb / fa for fa, fb in zip(fps["A"], fps["B"])]
    low, high = min(ratios), max(ratios)
    print(f"median: A {statistics.median(fps['A']):.1f} fps, B {statistics.median(fps['B']):.1f} fps")
    print(f"B/A: {statistics.median(ratios):.3f} (rounds {low:.3f}-{high:.3f})")
    if low > 1:
        print("B is faster in every round")
    elif high < 1:
        print("B is slower in every round")
    else:
        print("no consistent difference")


if __name__ == "__main__":
    main()
