"""Adapter regressions with original synthetic tarballs; no network or mGBA source."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

import prepare_mgba_reference as adapter


def tarball(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.data = tarball({"mgba-0.0.0/CMakeLists.txt": b"original placeholder\n"})
        self.lock = self.root / "lock.json"
        self.lock.write_text(json.dumps({
            "url": "https://example.invalid/mgba.tar.gz",
            "bytes": len(self.data),
            "sha256": hashlib.sha256(self.data).hexdigest(),
        }), encoding="utf-8")
        patcher = mock.patch.object(adapter, "LOCK", self.lock)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_verified_download_is_extracted_without_building(self):
        output = adapter.prepare(self.root / "out", download=lambda url: self.data, build=False)
        self.assertTrue((output / "src" / "mgba-0.0.0" / "CMakeLists.txt").is_file())

    def test_hash_and_size_mismatches_are_rejected_before_writing(self):
        for data in (self.data + b"x", self.data[:-1] + bytes([self.data[-1] ^ 1])):
            with self.assertRaises(ValueError):
                adapter.prepare(self.root / "out", download=lambda url: data, build=False)
            self.assertFalse((self.root / "out").exists())

    def test_existing_or_tracked_destinations_are_rejected(self):
        (self.root / "taken").mkdir()
        with self.assertRaises(ValueError):
            adapter.prepare(self.root / "taken", download=lambda url: self.data, build=False)
        with self.assertRaises(ValueError):
            adapter.prepare(adapter.ROOT / "crates" / "mgba", download=lambda url: self.data,
                            build=False)

    def test_unsafe_archive_members_are_rejected(self):
        data = tarball({"../escape.txt": b"no"})
        self.lock.write_text(json.dumps({
            "url": "https://example.invalid/mgba.tar.gz",
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        }), encoding="utf-8")
        with self.assertRaises(tarfile.TarError):
            adapter.prepare(self.root / "out", download=lambda url: data, build=False)
        self.assertFalse((self.root / "escape.txt").exists())


if __name__ == "__main__":
    unittest.main()
