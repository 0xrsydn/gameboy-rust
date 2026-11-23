"""Adapter regressions with original synthetic archives; no network or suite files."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

import prepare_public_test_roms as adapter


def archive(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


SUITE = archive({
    "suite-0000/arm/arm.gba": b"original rom",
    "suite-0000/arm/arm.asm": b"; not kept",
    "suite-0000/LICENSE": b"MIT",
    "suite-0000/README.md": b"not kept",
})


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.lock = self.root / "lock.json"
        patcher = mock.patch.object(adapter, "LOCK", self.lock)
        patcher.start()
        self.addCleanup(patcher.stop)

    def use(self, data):
        self.lock.write_text(json.dumps({"suites": [{
            "name": "suite", "project": "owner/suite", "url": "https://example.invalid/s.tar.gz",
            "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
        }]}))

    def test_only_roms_and_licenses_are_kept(self):
        self.use(SUITE)
        output, counts = adapter.prepare(self.root / "out", download=lambda url: SUITE)
        self.assertEqual(counts, {"suite": 1})
        self.assertTrue((output / "suite" / "arm" / "arm.gba").is_file())
        self.assertTrue((output / "suite" / "LICENSE").is_file())
        self.assertFalse((output / "suite" / "arm" / "arm.asm").exists())
        self.assertFalse((output / "suite" / "README.md").exists())
        self.assertFalse((output / "suite.tar.gz").exists())

    def test_mismatches_and_unsafe_members_are_rejected(self):
        self.use(SUITE)
        with self.assertRaises(ValueError):
            adapter.prepare(self.root / "a", download=lambda url: SUITE + b"x")
        self.assertFalse((self.root / "a").exists())
        bad = archive({"suite-0000/../../escape.gba": b"no"})
        self.use(bad)
        with self.assertRaises((tarfile.TarError, ValueError, OSError)):
            adapter.prepare(self.root / "b", download=lambda url: bad)
        self.assertFalse((self.root / "escape.gba").exists())


if __name__ == "__main__":
    unittest.main()
