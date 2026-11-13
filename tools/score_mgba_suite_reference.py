#!/usr/bin/env python3
"""Run the mGBA suite ROM inside mgba-ref and print the suite's own counters.
usage: score_mgba_suite_reference.py MGBA_REF ROM [SUITE_INDEX ...]"""
import struct
import subprocess
import sys

TABLE, COUNT = 0x0803D868, 14
A, DOWN = 0x001, 0x080


class Ref:
    def __init__(self, exe, rom):
        self.p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.cmd(f"load {rom}")

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


def main():
    exe, rom = sys.argv[1], sys.argv[2]
    image = open(rom, "rb").read()
    word = lambda a: struct.unpack_from("<I", image, a - 0x08000000)[0]

    def text(a):
        o = a - 0x08000000
        return image[o:image.index(b"\0", o)].decode()

    wanted = [int(x) for x in sys.argv[3:]] or range(COUNT)
    for index in wanted:
        entry = word(TABLE + index * 4)
        name, passes, total = text(word(entry)), word(entry + 20), word(entry + 24)
        if not (0x03000000 <= total < 0x03008000):
            print(f"{index} {name}: visual only")
            continue
        ref = Ref(exe, rom)
        ref.frames(60)
        for _ in range(index):
            ref.tap(DOWN)
        ref.tap(A)
        previous, stable, frames = 0, 0, 0
        while frames < 6000:
            now = struct.unpack("<I", ref.read(total, 4))[0]
            stable = stable + 1 if now == previous else 0
            previous = now
            if now and stable >= 240:
                break
            ref.frames(1)
            frames += 1
        got = struct.unpack("<I", ref.read(passes, 4))[0]
        print(f"{index} {name}: {got} / {previous} ({frames} frames)")
        sram = ref.read(0x0E000000, 0x8000).split(b"\0")[0].decode(errors="replace")
        test = ""
        for line in sram.splitlines()[2:]:
            if ": FAIL" in line:
                print(f"    {test} | {line.rstrip()}")
            elif " test: " in line:
                test = line.split(" test: ", 1)[1].rstrip()
        ref.close()


main()
