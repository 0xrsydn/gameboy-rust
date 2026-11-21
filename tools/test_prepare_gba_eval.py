"""Adapter regressions with original synthetic archives; no network or benchmark files."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

import prepare_gba_eval as adapter

TOP = "gba-eval-0000000/"
STUB = bytes(range(256)) * 64  # 16 KiB, original filler
ROM = b"original test cartridge"


def archive(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def cases(*hashes):
    return json.dumps({"testcases": [
        {"id": f"case{index}", "rom_sha256": digest} for index, digest in enumerate(hashes)
    ]}).encode()


def layout(testcases):
    return {
        TOP + "spec/gba_bios_stub.bin": STUB,
        TOP + "spec/LICENSE": b"MIT",
        TOP + "corpus/testcases.json": testcases,
        TOP + "corpus/replays/case0.txt": b"0 0000\n",
        TOP + "corpus/roms/test/case.gba": ROM,
        TOP + "harness/grader/src/main.rs": b"// not kept\n",
        TOP + "reference/mesen.wasm": b"not kept",
    }


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.lock = self.root / "lock.json"
        patcher = mock.patch.object(adapter, "LOCK", self.lock)
        patcher.start()
        self.addCleanup(patcher.stop)

    def use(self, data, stub=STUB):
        self.lock.write_text(json.dumps({
            "url": "https://example.invalid/gba-eval.tar.gz",
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "bios_sha256": hashlib.sha256(stub).hexdigest(),
        }))
        return data

    def test_only_the_stub_licenses_and_corpus_are_kept(self):
        data = self.use(archive(layout(cases(hashlib.sha256(ROM).hexdigest()))))
        output, count, missing = adapter.prepare(self.root / "out", download=lambda url: data)
        self.assertEqual((output / "gba_bios_stub.bin").read_bytes(), STUB)
        self.assertTrue((output / "corpus" / "roms" / "test" / "case.gba").is_file())
        self.assertTrue((output / "corpus" / "replays" / "case0.txt").is_file())
        self.assertFalse((output / "harness").exists())
        self.assertFalse((output / "reference").exists())
        self.assertFalse((output / "gba-eval.tar.gz").exists())
        self.assertEqual((count, missing), (1, []))

    def test_testcases_without_their_rom_are_reported(self):
        data = self.use(archive(layout(cases(hashlib.sha256(ROM).hexdigest(), "00" * 32))))
        _, count, missing = adapter.prepare(self.root / "out", download=lambda url: data)
        self.assertEqual((count, missing), (2, ["case1"]))

    def test_archive_and_stub_mismatches_are_rejected(self):
        data = self.use(archive(layout(cases())))
        with self.assertRaises(ValueError):
            adapter.prepare(self.root / "a", download=lambda url: data + b"x")
        self.assertFalse((self.root / "a").exists())
        data = self.use(archive(layout(cases())), stub=b"other")
        with self.assertRaises(ValueError):
            adapter.prepare(self.root / "b", download=lambda url: data)

    def test_unsafe_members_and_tracked_destinations_are_rejected(self):
        data = self.use(archive({TOP + "corpus/roms/../../escape.gba": ROM}))
        with self.assertRaises((tarfile.TarError, ValueError, OSError)):
            adapter.prepare(self.root / "out", download=lambda url: data)
        self.assertFalse((self.root / "escape.gba").exists())
        with self.assertRaises(ValueError):
            adapter.prepare(Path(adapter.__file__).resolve().parent.parent / "docs" / "x",
                            download=lambda url: data)


if __name__ == "__main__":
    unittest.main()
