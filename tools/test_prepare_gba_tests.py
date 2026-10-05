"""Adapter regressions with original synthetic bytes; no network or public ROMs."""
import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import prepare_gba_tests as adapter


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.output = self.root / "prepared"
        self.files = {
            "arm/arm.gba": struct.pack("<II", 0xEAFFFFFE, 0xE92D0003),
            "LICENSE": b"Original fixture, not upstream license content.\n",
            "arm/arm.asm": b"Original fixture source marker.\n",
        }
        self.lock = {
            "repository": "original/test-fixture",
            "revision": "0" * 40,
            "license": "fixture",
            "rom": "arm/arm.gba",
            "completion_pc": "0x08000004",
            "result_register": 12,
            "files": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in self.files.items()},
        }
        self.lock_path = self.root / "lock.json"
        self.save_lock()
        patch = mock.patch.object(adapter, "LOCK", self.lock_path)
        patch.start()
        self.addCleanup(patch.stop)

    def save_lock(self):
        self.lock_path.write_text(json.dumps(self.lock), encoding="utf-8")

    def download(self, url):
        prefix = "https://raw.githubusercontent.com/original/test-fixture/" + "0" * 40 + "/"
        self.assertTrue(url.startswith(prefix))
        return self.files[url[len(prefix):]]

    def prepare(self):
        return adapter.prepare(self.output, download=self.download)

    def test_verified_bytes_license_and_provenance_are_retained(self):
        manifest_path = self.prepare()
        for name, expected in self.files.items():
            self.assertEqual((self.output / name).read_bytes(), expected)
        self.assertEqual(json.loads((self.output / "source-lock.json").read_text()), self.lock)
        manifest = json.loads(manifest_path.read_text())
        case = manifest["cases"][0]
        self.assertEqual(case["completion"], {"pc": "0x08000004", "instruction_set": "arm"})
        self.assertEqual(case["checks"], [{"kind": "register", "index": 12, "equals": 0}])
        self.assertEqual(case["rom"], "arm/arm.gba")
        self.assertEqual(case["step_limit"], 1_000_000)

    def test_existing_directory_is_rejected_before_fetching(self):
        self.output.mkdir()
        sentinel = self.output / "keep"
        sentinel.write_text("unchanged")
        with self.assertRaisesRegex(ValueError, "already exists"):
            adapter.prepare(self.output, download=lambda _: self.fail("unexpected download"))
        self.assertEqual(sentinel.read_text(), "unchanged")

    def test_failed_hash_or_size_leaves_no_output(self):
        for replacement in [b"short", b"x" * len(self.files["arm/arm.gba"])]:
            with self.subTest(replacement=replacement):
                self.files["arm/arm.gba"] = replacement
                with self.assertRaisesRegex(ValueError, "mismatch"):
                    self.prepare()
                self.assertFalse(self.output.exists())

    def test_bad_checkpoint_is_rejected_even_with_matching_hash(self):
        self.lock["completion_pc"] = "0x08000000"
        self.save_lock()
        with self.assertRaisesRegex(ValueError, "checkpoint"):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_offline_tree_gets_the_same_checks_and_no_network(self):
        source = self.root / "source"
        for name, data in self.files.items():
            target = source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        adapter.prepare(self.output, source, download=lambda _: self.fail("unexpected network"))
        self.assertEqual((self.output / "arm/arm.gba").read_bytes(), self.files["arm/arm.gba"])
        (source / "LICENSE").write_bytes(b"modified")
        with self.assertRaisesRegex(ValueError, "mismatch"):
            adapter.prepare(self.root / "second", source)
        self.assertFalse((self.root / "second").exists())

    def test_download_failure_does_not_publish_partial_fixture(self):
        calls = 0
        def download(url):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("network unavailable")
            return self.download(url)
        with self.assertRaisesRegex(OSError, "network unavailable"):
            adapter.prepare(self.output, download=download)
        self.assertFalse(self.output.exists())

    def test_tracked_repository_paths_are_rejected(self):
        fake_repo = self.root / "repo"
        fake_repo.mkdir()
        (fake_repo / "roms").mkdir()
        with mock.patch.object(adapter, "ROOT", fake_repo):
            with self.assertRaisesRegex(ValueError, "outside the repository"):
                adapter.destination(fake_repo / "assets")
            self.assertEqual(adapter.destination(fake_repo / "roms" / "local"), fake_repo / "roms" / "local")

    def test_file_capacity_is_independent_of_declared_size(self):
        data = b"x" * (adapter.MAX_FILE_BYTES + 1)
        expected = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        with self.assertRaisesRegex(ValueError, "size mismatch"):
            adapter.verify("fixture", data, expected)


if __name__ == "__main__":
    unittest.main()
