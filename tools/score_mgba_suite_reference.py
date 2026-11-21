#!/usr/bin/env python3
"""Run the mGBA suite ROM inside a reference shim and print the suite's own counters.

usage: score_mgba_suite_reference.py [--bios BIOS] SHIM ROM [SUITE_INDEX ...]

SHIM is mgba-ref or mesen-ref. mesen-ref has no high-level BIOS and needs --bios, a 16 KiB
firmware image (this emulator's, for example). The menu presses, settle rule, and SRAM failure
log follow crates/gba-diff/examples/mgba_suite_scoreboard.rs. For the visual sub-suite, each
screen's "actual" picture is compared with the "expected" one that the same emulator draws."""
import struct
import subprocess
import sys

TABLE, COUNT = 0x0803D868, 14
A, START, RIGHT, DOWN = 0x001, 0x008, 0x010, 0x080
BOOT_FRAMES, SETTLE_FRAMES, FRAME_CAP, VIDEO_SETTLE_FRAMES = 60, 240, 18000, 20
IWRAM = range(0x03000000, 0x03008000)


class Ref:
    def __init__(self, exe, rom, bios=None):
        self.p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.cmd(f"load {rom}" + (f" {bios}" if bios else ""))

    def cmd(self, text):
        self.p.stdin.write((text + "\n").encode())
        self.p.stdin.flush()
        line = self.p.stdout.readline().decode().strip()
        if not line.startswith("ok"):
            raise RuntimeError(f"{text!r} -> {line!r}")
        return line

    def read(self, address, length):
        n = int(self.cmd(f"read {address:#x} {length}").split()[1])
        return self.p.stdout.read(n)

    def word(self, address):
        return struct.unpack("<I", self.read(address, 4))[0]

    def framebuffer(self):
        width, height = (int(x) for x in self.cmd("fb").split()[1:3])
        return self.p.stdout.read(width * height * 4)

    def frames(self, count):
        for _ in range(count):
            self.cmd("frame")

    def tap(self, mask):
        self.cmd(f"keys {mask}")
        self.frames(2)
        self.cmd("keys 0")
        self.frames(2)

    def close(self):
        self.p.stdin.write(b"quit\n")
        self.p.stdin.flush()
        self.p.wait()


def failures(sram):
    """Mismatch lines from the suite's SRAM log, each prefixed with its test's name."""
    test, found = "", []
    for line in sram.splitlines()[2:]:
        if ": FAIL" in line:
            found.append(f"{test} | {line.rstrip()}" if test else line.rstrip())
        elif " test: " in line:
            test = line.split(" test: ", 1)[1].rstrip()
    return found


def score_counted(ref, index, passes, total):
    ref.frames(BOOT_FRAMES)
    for _ in range(index):
        ref.tap(DOWN)
    ref.tap(A)
    previous, stable, frames = 0, 0, 0
    while frames < FRAME_CAP:
        now = ref.word(total)
        stable = stable + 1 if now == previous else 0
        previous = now
        if now and stable >= SETTLE_FRAMES:
            break
        ref.frames(1)
        frames += 1
    sram = ref.read(0x0E000000, 0x8000).split(b"\0")[0].decode(errors="replace")
    return ref.word(passes), previous, frames, failures(sram)


def score_screen(ref, index, test):
    """Pixels in which the screen's actual picture differs from its expected one."""
    ref.frames(BOOT_FRAMES)
    for _ in range(index):
        ref.tap(DOWN)
    ref.tap(A)
    ref.frames(VIDEO_SETTLE_FRAMES)
    for _ in range(test):
        ref.tap(DOWN)
    listing = ref.framebuffer()
    for key in (A, START):  # Open the screen, then hide its "actual"/"expected" label.
        ref.tap(key)
        ref.frames(VIDEO_SETTLE_FRAMES)
    actual = ref.framebuffer()
    if actual == listing:
        raise RuntimeError("the screen did not open")
    ref.tap(RIGHT)
    ref.frames(VIDEO_SETTLE_FRAMES)
    expected = ref.framebuffer()
    return sum(actual[i:i + 4] != expected[i:i + 4] for i in range(0, len(actual), 4))


def main(args):
    bios = None
    if args[:1] == ["--bios"]:
        bios, args = args[1], args[2:]
    if len(args) < 2:
        sys.exit(__doc__.splitlines()[2])
    exe, rom = args[0], args[1]
    image = open(rom, "rb").read()
    word = lambda a: struct.unpack_from("<I", image, a - 0x08000000)[0]

    def text(a):
        o = a - 0x08000000
        return image[o:image.index(b"\0", o)].decode()

    for index in [int(x) for x in args[2:]] or range(COUNT):
        entry = word(TABLE + index * 4)
        name, tests = text(word(entry)), word(entry + 16)
        passes, total = word(entry + 20), word(entry + 24)
        if total not in IWRAM:
            differing = []
            for test in range(tests):
                ref = Ref(exe, rom, bios)
                pixels = score_screen(ref, index, test)
                ref.close()
                if pixels:
                    differing.append(f"screen {test}: {pixels} pixels differ")
            print(f"{index} {name}: {tests - len(differing)} / {tests} screens identical")
            for line in differing:
                print(f"    {line}")
            continue
        ref = Ref(exe, rom, bios)
        got, results, frames, lines = score_counted(ref, index, passes, total)
        ref.close()
        print(f"{index} {name}: {got} / {results} ({frames} frames)")
        for line in lines:
            print(f"    {line}")


if __name__ == "__main__":
    main(sys.argv[1:])
