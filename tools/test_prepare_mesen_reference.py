"""Adapter regressions with original synthetic tarballs; no network or Mesen source."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

import prepare_mesen_reference as adapter


def tarball(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


ROOT = "Mesen2-gba-headless-0000000/"
LAYOUT = {
    ROOT + "Core/GBA/GbaCpu.cpp": b"// placeholder\n",
    ROOT + "Core/GBA/GbaCpu.h": b"// placeholder\n",
    ROOT + "Core/Shared/Emulator.cpp": b"// placeholder\n",
    ROOT + "Utilities/miniz.c": b"/* placeholder */\n",
    ROOT + "Core/Debugger/Debugger.cpp": b"// placeholder\n",
    ROOT + "Core/Debugger/LuaApi.cpp": b"// placeholder\n",
    ROOT + "Core/Debugger/Disassembler.cpp": b"// placeholder\n",
    ROOT + "Core/NES/NesCpu.cpp": b"// placeholder\n",
}


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.data = tarball(LAYOUT)
        self.lock = self.root / "lock.json"
        self.write_lock(self.data)
        patcher = mock.patch.object(adapter, "LOCK", self.lock)
        patcher.start()
        self.addCleanup(patcher.stop)

    def write_lock(self, data):
        self.lock.write_text(json.dumps({
            "url": "https://example.invalid/mesen.tar.gz",
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        }), encoding="utf-8")

    def test_verified_download_is_extracted_without_building(self):
        output = adapter.prepare(self.root / "out", download=lambda url: self.data, build=False)
        self.assertTrue((output / "src" / ROOT / "Core" / "GBA" / "GbaCpu.cpp").is_file())

    def test_core_sources_skip_headers_other_cores_and_stubbed_debugger_files(self):
        output = adapter.prepare(self.root / "out", download=lambda url: self.data, build=False)
        source = output / "src" / ROOT
        names = sorted(str(path.relative_to(source)) for path in adapter.sources(source))
        self.assertEqual(names, [
            "Core/Debugger/Debugger.cpp",
            "Core/GBA/GbaCpu.cpp",
            "Core/Shared/Emulator.cpp",
            "Utilities/miniz.c",
        ])

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
            adapter.prepare(adapter.SHIM_DIR, download=lambda url: self.data, build=False)

    def test_unsafe_archive_members_are_rejected(self):
        data = tarball({"../escape.txt": b"no"})
        self.write_lock(data)
        with self.assertRaises(tarfile.TarError):
            adapter.prepare(self.root / "out", download=lambda url: data, build=False)
        self.assertFalse((self.root / "escape.txt").exists())


if __name__ == "__main__":
    unittest.main()
