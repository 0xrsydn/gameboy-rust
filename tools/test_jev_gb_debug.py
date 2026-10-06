"""Offline tests. No provider request or real credential is needed."""
import copy
import io
import json
import math
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import jev_gb_debug as jev

FIXTURES = Path(__file__).parent / "fixtures" / "jev"


def evidence():
    return jev.read_json(FIXTURES / "audio-blocker.json")


def response(context):
    answers = {}
    for name, question in context["questions"].items():
        if question["type"] == "bool":
            answers[name] = {"type": "bool", "probability": 0.01}
        else:
            selected = "audio" if name == "subsystem" else "diagnostic"
            answers[name] = {"type": "choice", "choice": selected, "confidence": 1,
                             "probabilities": {key: int(key == selected) for key in question["criteria"]}}
    return {"model": "jev-test", "stopReason": "stop", "answers": answers, "usage": {"input": 1, "output": 1}}


class JevTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.env = patch.dict(os.environ, {}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_fixtures_and_rubric_are_bounded_and_original(self):
        for path in FIXTURES.glob("*.json"):
            context = jev.context(jev.read_json(path))
            self.assertEqual(set(context), {"state", "questions"})
            self.assertLess(len(jev.encoded(context)), 16384)
            self.assertIn("unknown", context["questions"]["subsystem"]["criteria"])

    def test_rejects_extra_fields_invalid_results_sizes_and_sensitive_text(self):
        invalids = []
        for key, value in [("rom_bytes", "private"), ("case_id", "../rom"), ("claim", ""),
                           ("observations", []), ("observations", ["x"] * 13), ("checks", [{}] * 25)]:
            item = evidence()
            item[key] = value
            invalids.append(item)
        for value in ["Authorization: private", "Bearer private", "/Users/person/private.log",
                      "API_KEY=private", "data:image/png;base64,secret", "x" * 1025]:
            item = evidence()
            item["observations"] = [value]
            invalids.append(item)
        item = evidence()
        item["checks"][0]["result"] = "maybe"
        invalids.append(item)
        item = evidence()
        item["checks"] *= 2
        invalids.append(item)
        for item in invalids:
            with self.assertRaises(ValueError):
                jev.context(item)
        with patch.dict(os.environ, {"TYPESAFEAI_API_KEY": "test-only-secret"}):
            item = evidence()
            item["claim"] = "test-only-secret"
            with self.assertRaises(ValueError):
                jev.context(item)

    def test_reader_rejects_binary_directory_oversized_and_nonfinite_json(self):
        for name, content in [("game.gba", b"1234"), ("bad.json", b"\xff"),
                              ("large.json", b" " * (jev.MAX_BYTES + 1)), ("nan.json", b'NaN')]:
            path = self.path / name
            path.write_bytes(content)
            with self.assertRaises((ValueError, UnicodeError)):
                jev.read_json(path)
        with self.assertRaises(ValueError):
            jev.read_json(self.path)

    def test_classifier_cannot_override_failed_checks_or_prove_compatibility(self):
        context = jev.context(evidence())
        result = response(context)
        result["answers"]["claim_supported"]["probability"] = 1
        summary = jev.summarize(context, result)
        self.assertEqual(summary["deterministic_checks"], "failed")
        self.assertEqual(summary["compatibility_verdict"], "not_established")
        self.assertEqual(summary["automatic_action"], "none")
        for state, expected in [("pass", "checks_passed"), ("not_run", "incomplete")]:
            context["state"]["checks"][0]["result"] = state
            self.assertEqual(jev.summarize(context, result)["deterministic_checks"], expected)
        result["answers"]["subsystem"]["confidence"] = 0.7
        self.assertTrue(jev.summarize(context, result)["review_required"])

    def test_validates_native_and_http_answer_types_and_probabilities(self):
        context = jev.context(evidence())
        result = response(context)
        result["answers"]["claim_supported"] = {"type": "noul", "noul": 0.01}
        jev.summarize(context, result)
        for mutate in [
            lambda r: r.update(stopReason="error"),
            lambda r: r.pop("model"),
            lambda r: r["answers"].pop("subsystem"),
            lambda r: r["answers"]["subsystem"].update(choice="run-a-shell"),
            lambda r: r["answers"]["subsystem"].update(confidence=math.nan),
            lambda r: r["answers"]["subsystem"].update(probabilities={}),
            lambda r: r["answers"]["subsystem"]["probabilities"].update(audio=0.5),
            lambda r: r["answers"]["claim_supported"].update(noul=True),
            lambda r: r["answers"]["claim_supported"].update(noul=2),
        ]:
            bad = copy.deepcopy(result)
            mutate(bad)
            with self.assertRaises(ValueError):
                jev.summarize(context, bad)

    def test_records_request_revision_hashes_response_and_nonoverwrite(self):
        context = jev.context(evidence())
        path = self.path / "record.json"
        with patch.object(jev, "revision", return_value="a" * 40):
            record = jev.evaluate(context, path, response(context))
        self.assertEqual(record["status"], "completed")
        self.assertEqual(json.loads(path.read_text()), record)
        self.assertEqual(record["request_sha256"], jev.digest(context))
        self.assertEqual(record["jj_commit"], "a" * 40)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            jev.evaluate(context, path, call=lambda _: self.fail("must not send"))
        with self.assertRaises(ValueError):
            jev.evaluate(context, jev.ROOT / "tracked-eval.json", call=lambda _: self.fail("must not send"))

    def test_provider_error_and_secret_echo_are_not_saved_as_success(self):
        context = jev.context(evidence())
        result = response(context)
        result.update(stopReason="error", errorMessage="private provider body")
        record = jev.evaluate(context, self.path / "failure.json", result)
        self.assertEqual(record["status"], "error")
        self.assertNotIn("private provider body", json.dumps(record))
        result = response(context)
        result["extra"] = "test-only-secret"
        with patch.dict(os.environ, {"TYPESAFEAI_API_KEY": "test-only-secret"}):
            record = jev.evaluate(context, self.path / "echo.json", result)
        self.assertEqual(record["status"], "error")
        self.assertNotIn("test-only-secret", json.dumps(record))

    def test_http_uses_fixed_endpoint_alias_key_and_noul_without_redirects(self):
        context = jev.context(evidence())
        payload = response(context)
        with patch.dict(os.environ, {"TYPESAFEAI_API_KEY": "test-only-secret"}), patch.object(jev, "build_opener") as opener:
            opener.return_value.open.return_value = io.BytesIO(json.dumps(payload).encode())
            self.assertEqual(jev.remote(context), payload)
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.full_url, jev.ENDPOINT)
            self.assertEqual(request.headers["Authorization"], "Bearer test-only-secret")
            self.assertEqual(json.loads(request.data)["questions"]["claim_supported"]["type"], "noul")
            self.assertEqual(context["questions"]["claim_supported"]["type"], "bool")
            self.assertEqual(opener.call_args.args, (jev.NoRedirect,))
        self.assertIsNone(jev.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other.invalid"))

    def test_missing_key_http_error_and_oversized_response_fail_without_retry(self):
        context = jev.context(evidence())
        with self.assertRaisesRegex(ValueError, "credential is unavailable"):
            jev.remote(context)
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": "test-only-secret"}), patch.object(jev, "build_opener") as opener:
            opener.return_value.open.side_effect = HTTPError(jev.ENDPOINT, 429, "private", {}, io.BytesIO(b"private"))
            with self.assertRaisesRegex(ValueError, "HTTP 429") as error:
                jev.remote(context)
            self.assertNotIn("private", str(error.exception))
            self.assertEqual(opener.return_value.open.call_count, 1)
            opener.return_value.open.side_effect = None
            opener.return_value.open.return_value = io.BytesIO(b" " * (jev.MAX_RESPONSE + 1))
            with self.assertRaisesRegex(ValueError, "response exceeds limit"):
                jev.remote(context)

    def test_pi_resources_and_ignored_log_directory_are_consistent(self):
        skill = jev.ROOT / ".pi/skills/jev-gb-debug/SKILL.md"
        prompt = jev.ROOT / ".pi/prompts/jev-gb-debug.md"
        self.assertIn("name: jev-gb-debug", skill.read_text())
        self.assertIn("description:", skill.read_text())
        self.assertIn(".pi/skills/jev-gb-debug/SKILL.md", prompt.read_text())
        self.assertIn("${@:-", prompt.read_text())
        self.assertTrue((skill.parent / "../../../docs/jev-debugging.md").resolve().is_file())
        settings = jev.read_json(jev.ROOT / ".pi/settings.json")
        self.assertIn("+codemode", settings["defaultTools"])
        self.assertIn("/.pi/jev-runs/", (jev.ROOT / ".gitignore").read_text().splitlines())

    def test_cli_preview_is_offline_and_null_import_cannot_trigger_network(self):
        path = FIXTURES / "audio-blocker.json"
        with patch.object(jev.sys, "argv", ["jev", str(path)]), patch.object(jev, "remote", side_effect=AssertionError("network")), patch("sys.stdout", new_callable=io.StringIO) as out:
            self.assertEqual(jev.main(), 0)
            self.assertEqual(json.loads(out.getvalue()), jev.context(evidence()))
        null = self.path / "null.json"
        null.write_text("null")
        with patch.object(jev.sys, "argv", ["jev", str(path), "--response", str(null), "--output", str(self.path / "out.json")]), patch.object(jev, "remote", side_effect=AssertionError("network")), patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(jev.main(), 1)
        self.assertFalse((self.path / "out.json").exists())


if __name__ == "__main__":
    unittest.main()
