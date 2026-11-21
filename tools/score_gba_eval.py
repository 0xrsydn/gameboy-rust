#!/usr/bin/env python3
"""Run the GBA Eval corpus through `gba-diff --eval` and summarize per testcase.

This adapter is original. For each testcase in CORPUS/corpus/testcases.json it finds the ROM by
SHA-256, runs `gba-diff --eval` against mesen-ref with the benchmark's BIOS stub, the testcase's
replay, and its frame count, and reports how many frames match Mesen pixel for pixel (after
5-bit quantization). `endstate` testcases are judged by their last frame, as the benchmark
judges them. This is a local proxy: the benchmark scores structural similarity per frame and
the audio as well, so a differing frame here may still score well there.

Prepare CORPUS with tools/prepare_gba_eval.py and mesen-ref with prepare_mesen_reference.py.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SUMMARY = re.compile(
    r"eval: (\d+) frames, (\d+) identical \(([\d.]+)%\), ([\d.]+)% of pixels differ; "
    r"last frame (\d+) pixels; first differing \(frame, pixels\): (.*)$")


def roms_by_hash(corpus):
    return {hashlib.sha256(path.read_bytes()).hexdigest(): path
            for path in (corpus / "corpus" / "roms").rglob("*.gba")}


def run_case(case, roms, corpus, gba_diff, reference, frames_cap):
    rom = roms.get(case["rom_sha256"])
    if rom is None:
        return {"id": case["id"], "error": "ROM not in corpus"}
    frames = case["frames"] if frames_cap is None else min(case["frames"], frames_cap)
    command = [str(gba_diff), "--reference", str(reference), "--eval",
               "--bios", str(corpus / "gba_bios_stub.bin"), "--rom", str(rom),
               "--frames", str(frames)]
    if case.get("replay"):
        command += ["--replay", str(corpus / "corpus" / "replays" / case["replay"])]
    result = subprocess.run(command, capture_output=True, text=True)
    stopped = next((line for line in result.stdout.splitlines() if "stopped:" in line), None)
    match = next(filter(None, map(SUMMARY.match, result.stdout.splitlines())), None)
    if match is None:
        return {"id": case["id"], "error": (result.stderr or result.stdout).strip()[-300:]}
    total, identical = int(match[1]), int(match[2])
    return {
        "id": case["id"],
        "section": case["section"],
        "subsystem": case["subsystem"],
        "mode": case.get("scoring_mode", "frame_mean"),
        "frames": total,
        "identical": identical,
        "pixels_differ_percent": float(match[4]),
        "last_frame_pixels": int(match[5]),
        "first_differing": match[6],
        "stopped": stopped,
    }


def verdict(row):
    if "error" in row:
        return "error"
    if row["mode"] == "endstate":
        return "match" if row["last_frame_pixels"] == 0 else "differs"
    return f"{100 * row['identical'] / max(row['frames'], 1):.1f}%"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", default=ROOT / "roms" / "gba-eval", type=Path)
    parser.add_argument("--reference", default=ROOT / "roms" / "mesen-reference" / "mesen-ref",
                        type=Path)
    parser.add_argument("--gba-diff", default=ROOT / "target" / "release" / "gba-diff", type=Path)
    parser.add_argument("--only", help="comma-separated testcase ids")
    parser.add_argument("--frames", type=int, help="cap each testcase's frame count")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    parser.add_argument("--json", type=Path, help="write the rows as JSON")
    args = parser.parse_args()

    cases = json.loads((args.corpus / "corpus" / "testcases.json").read_text())["testcases"]
    if args.only:
        wanted = set(args.only.split(","))
        cases = [case for case in cases if case["id"] in wanted]
    roms = roms_by_hash(args.corpus)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        rows = list(pool.map(lambda case: run_case(case, roms, args.corpus, args.gba_diff,
                                                    args.reference, args.frames), cases))

    print(f"{'testcase':26} {'mode':10} {'frames':>6} {'identical':>9} {'verdict':>8}  notes")
    for row in rows:
        if "error" in row:
            print(f"{row['id']:26} {'':10} {'':>6} {'':>9} {'error':>8}  {row['error']}")
            continue
        note = row["stopped"] or (f"first differing {row['first_differing']}"
                                  if row["identical"] < row["frames"] else "")
        print(f"{row['id']:26} {row['mode']:10} {row['frames']:6} {row['identical']:9} "
              f"{verdict(row):>8}  {note[:110]}")
    scored = [row for row in rows if "error" not in row]
    endstate = [row for row in scored if row["mode"] == "endstate"]
    replays = [row for row in scored if row["mode"] != "endstate"]
    if endstate:
        print(f"endstate: {sum(verdict(row) == 'match' for row in endstate)}/{len(endstate)} "
              "final frames identical")
    if replays:
        frames = sum(row["frames"] for row in replays)
        identical = sum(row["identical"] for row in replays)
        print(f"replays: {identical}/{frames} frames identical ({100 * identical / frames:.1f}%)")
    if args.json:
        args.json.write_text(json.dumps(rows, indent=2) + "\n")
    return 0 if all("error" not in row for row in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
