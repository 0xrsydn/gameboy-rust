"""Adapter regressions with original synthetic bytes; no network or public ROMs."""
import hashlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import prepare_gba_tests as adapter


class PreparationTests(unittest.TestCase):
    suite = "arm"

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.output = self.root / "prepared"
        self.rom = f"{self.suite}/{self.suite}.gba"
        if self.suite == "arm":
            data = struct.pack("<II", 0xEAFFFFFE, 0xE92D0003)
            pc, result = "0x08000004", 12
        elif self.suite == "thumb":
            data = struct.pack("<IIHHI", 0xEAFFFFFE, 0xE1A00000, 0xA000, 0x4700, 0xE92D0003)
            pc, result = "0x0800000c", 7
        elif self.suite == "memory":
            data = struct.pack("<III", 0xEAFFFFFE, 0xE1A00000, 0xE92D0003)
            pc, result = "0x08000008", 12
        else:
            data = struct.pack("<IIII", 0xEAFFFFFE, 0xE1A00000, 0xE1A00000, 0xE92D0003)
            pc, result = "0x0800000c", 12
        self.files = {
            self.rom: data,
            "LICENSE": b"Original fixture, not upstream license content.\n",
            f"{self.suite}/{self.suite}.asm": b"Original fixture source marker.\n",
        }
        self.lock = {
            "repository": "original/test-fixture",
            "revision": "0" * 40,
            "license": "fixture",
            "rom": self.rom,
            "completion_pc": pc,
            "result_register": result,
            "files": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in self.files.items()},
        }
        self.lock_path = self.root / "lock.json"
        self.save_lock()
        # The unselected path does not exist. Reading the wrong lock must fail.
        locks = {suite: self.root / f"missing-{suite}.json" for suite in adapter.LOCKS}
        locks[self.suite] = self.lock_path
        patch = mock.patch.object(adapter, "LOCKS", locks)
        patch.start()
        self.addCleanup(patch.stop)

    def save_lock(self):
        self.lock_path.write_text(json.dumps(self.lock), encoding="utf-8")

    def download(self, url):
        prefix = "https://raw.githubusercontent.com/original/test-fixture/" + "0" * 40 + "/"
        self.assertTrue(url.startswith(prefix))
        return self.files[url[len(prefix):]]

    def prepare(self):
        # Exercise default ARM preparation and explicit selection of the other suites.
        options = {} if self.suite == "arm" else {"suite": self.suite}
        return adapter.prepare(self.output, download=self.download, **options)

    def offline_source(self):
        source = self.root / "source"
        for name, data in self.files.items():
            target = source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        return source

    def test_verified_bytes_license_and_provenance_are_retained(self):
        manifest_path = self.prepare()
        for name, expected in self.files.items():
            self.assertEqual((self.output / name).read_bytes(), expected)
        self.assertEqual(json.loads((self.output / "source-lock.json").read_text()), self.lock)
        manifest = json.loads(manifest_path.read_text())
        self.assertEqual(manifest, {
            "version": 1,
            "cases": [{
                "name": f"jsmolka-{self.suite}",
                "rom": self.rom,
                "step_limit": 1_000_000,
                # The Thumb ROM returns to ARM before evaluation.
                "completion": {"pc": self.lock["completion_pc"], "instruction_set": "arm"},
                "checks": [{"kind": "register", "index": self.lock["result_register"], "equals": 0}],
            }],
        })

    def test_existing_directory_is_rejected_before_fetching(self):
        self.output.mkdir()
        sentinel = self.output / "keep"
        sentinel.write_text("unchanged")
        with self.assertRaisesRegex(ValueError, "already exists"):
            adapter.prepare(self.output, download=lambda _: self.fail("unexpected download"), suite=self.suite)
        self.assertEqual(sentinel.read_text(), "unchanged")

    def test_failed_hash_or_size_leaves_no_output(self):
        for replacement in [b"short", b"x" * len(self.files[self.rom])]:
            with self.subTest(replacement=replacement):
                self.files[self.rom] = replacement
                with self.assertRaisesRegex(ValueError, "mismatch"):
                    self.prepare()
                self.assertFalse(self.output.exists())

    def test_bad_checkpoint_is_rejected_even_with_matching_hash(self):
        for pc in ["0x07fffffc", "0x08000000", "0x08000003", "0x08001000"]:
            with self.subTest(pc=pc):
                self.lock["completion_pc"] = pc
                self.save_lock()
                with self.assertRaisesRegex(ValueError, "checkpoint"):
                    self.prepare()
                self.assertFalse(self.output.exists())

    def test_offline_tree_gets_the_same_checks_and_no_network(self):
        source = self.offline_source()
        adapter.prepare(self.output, source, download=lambda _: self.fail("unexpected network"), suite=self.suite)
        self.assertEqual((self.output / self.rom).read_bytes(), self.files[self.rom])
        (source / "LICENSE").write_bytes(b"modified")
        with self.assertRaisesRegex(ValueError, "mismatch"):
            adapter.prepare(self.root / "second", source, suite=self.suite)
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
            adapter.prepare(self.output, download=download, suite=self.suite)
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

    def test_cli_prepares_offline_and_reports_errors_without_network(self):
        source = self.offline_source()
        argv = ["prepare_gba_tests.py", str(self.output), "--source-dir", str(source)]
        if self.suite != "arm":
            argv += ["--suite", self.suite]
        with mock.patch("sys.argv", argv), mock.patch("sys.stdout", new_callable=io.StringIO) as stdout:
            self.assertEqual(adapter.main(), 0)
            self.assertIn(f"public {self.suite} suite", stdout.getvalue())
        # Existing output is an error, not an overwrite or a successful preparation.
        with mock.patch("sys.argv", argv), mock.patch("sys.stderr", new_callable=io.StringIO) as stderr:
            self.assertEqual(adapter.main(), 1)
            self.assertIn("already exists", stderr.getvalue())


class ThumbPreparationTests(PreparationTests):
    suite = "thumb"

    def test_thumb_bridge_is_checked_even_with_matching_hash(self):
        original = self.files[self.rom]
        # Wrong ADR, wrong BX, or no bridge before an otherwise valid eval word.
        for data, pc in [
            (original[:8] + struct.pack("<HHI", 0xA001, 0x4700, 0xE92D0003), "0x0800000c"),
            (original[:8] + struct.pack("<HHI", 0xA000, 0x4708, 0xE92D0003), "0x0800000c"),
            (struct.pack("<I", 0xE92D0003), "0x08000000"),
        ]:
            with self.subTest(data=data):
                self.files[self.rom] = data
                self.lock["files"][self.rom] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                self.lock["completion_pc"] = pc
                self.save_lock()
                with self.assertRaisesRegex(ValueError, "Thumb-to-ARM checkpoint bridge"):
                    self.prepare()
                self.assertFalse(self.output.exists())


class MemoryPreparationTests(PreparationTests):
    suite = "memory"


class BiosPreparationTests(PreparationTests):
    suite = "bios"


class SuiteSelectionTests(unittest.TestCase):
    def test_unknown_suite_is_rejected_before_download_or_output(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "prepared"
            with self.assertRaisesRegex(ValueError, "unsupported public suite"):
                adapter.prepare(output, suite="../unknown", download=lambda _: self.fail("unexpected network"))
            self.assertFalse(output.exists())
            with mock.patch("sys.argv", ["prepare_gba_tests.py", str(output), "--suite", "unknown"]):
                with mock.patch("sys.stderr", new_callable=io.StringIO), self.assertRaises(SystemExit) as exit:
                    adapter.main()
                self.assertEqual(exit.exception.code, 2)
            self.assertFalse(output.exists())

    def test_committed_locks_keep_distinct_verified_protocols(self):
        for suite, pc, register in [
            ("arm", "0x08001d4c", 12),
            ("thumb", "0x08000934", 7),
            ("memory", "0x08000350", 12),
            ("bios", "0x08000248", 12),
        ]:
            lock = json.loads(adapter.LOCKS[suite].read_text())
            self.assertEqual(lock["repository"], "jsmolka/gba-tests")
            self.assertEqual(lock["revision"], "a7113b67e63f83a9b321696ddd7042ccfad6c881")
            self.assertEqual(lock["license"], "MIT")
            self.assertEqual(lock["completion_pc"], pc)
            self.assertEqual(lock["result_register"], register)
            self.assertEqual(lock["rom"], f"{suite}/{suite}.gba")
            self.assertIn(lock["rom"], lock["files"])
            self.assertIn("LICENSE", lock["files"])
            for name, metadata in lock["files"].items():
                self.assertFalse(Path(name).is_absolute())
                self.assertNotIn("..", Path(name).parts)
                self.assertRegex(metadata["sha256"], r"^[0-9a-f]{64}$")
                self.assertTrue(0 < metadata["bytes"] <= adapter.MAX_FILE_BYTES)


if __name__ == "__main__":
    unittest.main()
