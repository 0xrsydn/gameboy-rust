"""Offline adapter tests. All fixture bytes are original synthetic data."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import prepare_homebrew_pong as adapter
from prepare_gba_tests import ROOT, MAX_FILE_BYTES


class PongPreparation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.output = self.base / "prepared"
        self.source = self.base / "source"
        self.source.mkdir()
        real = json.loads(adapter.LOCK.read_text())
        self.files = {name: f"original fixture: {name}\n".encode() for name in real["files"]}
        self.lock = dict(real, files={name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                                     for name, data in self.files.items()})
        self.lock_path = self.base / "lock.json"
        self.lock_path.write_text(json.dumps(self.lock))
        self.addCleanup(patch.stopall)
        patch.object(adapter, "LOCK", self.lock_path).start()
        for name, data in self.files.items():
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    def test_offline_preparation_retains_license_and_is_repeatable(self):
        first = adapter.prepare(self.output, self.source)
        second = adapter.prepare(self.base / "second", self.source)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        self.assertEqual(json.loads(first.read_text()), adapter.manifest(self.lock["rom"]))
        self.assertEqual(json.loads((self.output / "source-lock.json").read_text()), self.lock)
        for name, data in self.files.items():
            self.assertEqual((self.output / name).read_bytes(), data)
        with self.assertRaisesRegex(ValueError, "already exists"):
            adapter.prepare(self.output, self.source)
        self.assertEqual((self.output / "LICENSE").read_bytes(), self.files["LICENSE"])

    def test_download_uses_only_pinned_revision_urls(self):
        base = f"https://raw.githubusercontent.com/{self.lock['repository']}/{self.lock['revision']}/"
        calls = []

        def download(url):
            self.assertTrue(url.startswith(base))
            calls.append(url.removeprefix(base))
            return self.files[calls[-1]]

        adapter.prepare(self.output, download=download)
        self.assertEqual(calls, list(self.files))

    def test_bad_hash_size_missing_and_oversized_files_leave_no_output(self):
        rom = self.source / self.lock["rom"]
        original = rom.read_bytes()
        for replacement in [bytes(len(original)), original + b"x", b"x" * (MAX_FILE_BYTES + 1)]:
            rom.write_bytes(replacement)
            with self.assertRaises(ValueError):
                adapter.prepare(self.output, self.source)
            self.assertFalse(self.output.exists())
        rom.unlink()
        with self.assertRaises(ValueError):
            adapter.prepare(self.output, self.source)
        self.assertFalse(self.output.exists())

    def test_network_failure_leaves_no_output(self):
        def fail(_):
            raise OSError("offline")
        with self.assertRaises(OSError):
            adapter.prepare(self.output, download=fail)
        self.assertFalse(self.output.exists())

    def test_tracked_and_symlinked_destinations_are_rejected_before_fetch(self):
        def fail(_):
            self.fail("must not fetch for unsafe destination")
        with self.assertRaises(ValueError):
            adapter.prepare(ROOT / "tracked-pong-test", download=fail)
        link = self.base / "linked"
        link.symlink_to(ROOT, target_is_directory=True)
        with self.assertRaises(ValueError):
            adapter.prepare(link / "tracked-pong-test", download=fail)
        with self.assertRaises(ValueError):
            adapter.prepare(self.base / "missing" / "out", download=fail)

    def test_manifest_has_bounded_distinct_scenarios_and_complete_button_snapshots(self):
        manifest = adapter.manifest("test.gba")
        self.assertEqual(manifest["version"], 2)
        cases = manifest["cases"]
        self.assertEqual(len({case["name"] for case in cases}), len(cases))
        self.assertLessEqual(sum(case["step_limit"] for case in cases), 100000000)
        for case in cases:
            self.assertEqual(case["rom"], "test.gba")
            frames = [event["vblank"] for event in case["inputs"]]
            self.assertEqual(frames, sorted(set(frames)))
            self.assertTrue(all(frame < case["completion"]["vblanks"] for frame in frames))
            for event in case["inputs"]:
                self.assertLessEqual(event["buttons"], 1023)
            if case["inputs"]:
                self.assertEqual(case["inputs"][-1]["buttons"], 0)
            self.assertTrue(case["checks"])
            for check in case["checks"]:
                self.assertEqual(check["kind"], "pixel")
                self.assertLess(check["x"], 240)
                self.assertLess(check["y"], 160)
                self.assertLessEqual(check["equals"], 0xffffff)


if __name__ == "__main__":
    unittest.main()
