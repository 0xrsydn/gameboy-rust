#!/usr/bin/env python3
"""Optional Jev evidence review. Preview by default; never opens ROMs or runs games."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, HTTPRedirectHandler, build_opener

ROOT = Path(__file__).resolve().parent.parent
RUBRIC = Path(__file__).with_name("jev-gb-rubric.json")
ENDPOINT = "https://api.typesafe.ai/v1/systemone"
MODEL = "jev-latest"
MAX_BYTES = 16 * 1024
MAX_RESPONSE = 64 * 1024


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def read_json(path, limit=MAX_BYTES):
    path = Path(path)
    if path.suffix != ".json" or not path.is_file() or path.stat().st_size > limit:
        raise ValueError("input must be a bounded regular .json file")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("input exceeds byte limit")
    return json.loads(data, parse_constant=lambda _: (_ for _ in ()).throw(ValueError("non-finite JSON")))


def text_field(value, limit):
    if not isinstance(value, str) or not value.strip() or len(value) > limit:
        raise ValueError("invalid or oversized evidence text")
    if re.search(r"(?i)bearer\s|api[_-]?key|authorization|(?:/Users/|/home/)|data:.*base64", value):
        raise ValueError("remove credentials, host paths, or binary payloads before evaluation")
    for name in ("TYPESAFEAI_API_KEY", "TYPESAFE_API_KEY"):
        secret = os.environ.get(name)
        if secret and secret in value:
            raise ValueError("credential found in evidence")


def context(evidence):
    if not isinstance(evidence, dict) or set(evidence) != {"case_id", "observations", "checks", "claim"}:
        raise ValueError("evidence requires only case_id, observations, checks, and claim")
    if not isinstance(evidence["case_id"], str) or not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,63}", evidence["case_id"]):
        raise ValueError("case_id must be a short lowercase identifier")
    text_field(evidence["claim"], 1024)
    observations, checks = evidence["observations"], evidence["checks"]
    if not isinstance(observations, list) or not 1 <= len(observations) <= 12:
        raise ValueError("supply 1..12 observations")
    for observation in observations:
        text_field(observation, 1024)
    if not isinstance(checks, list) or len(checks) > 24:
        raise ValueError("supply at most 24 checks")
    names = set()
    for check in checks:
        if not isinstance(check, dict) or set(check) != {"name", "result", "scope"}:
            raise ValueError("each check requires name, result, and scope")
        text_field(check["name"], 128)
        text_field(check["scope"], 512)
        if check["name"] in names or check["result"] not in ("pass", "fail", "not_run"):
            raise ValueError("check names must be unique; result must be pass, fail, or not_run")
        names.add(check["name"])
    request = {"state": evidence, "questions": read_json(RUBRIC)["questions"]}
    if len(encoded(evidence)) > MAX_BYTES:
        raise ValueError("evidence exceeds byte limit")
    return request


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None  # Never forward credentials to a redirected endpoint.


def remote(context):
    key = os.environ.get("TYPESAFEAI_API_KEY") or os.environ.get("TYPESAFE_API_KEY")
    if not key:
        raise ValueError("TypeSafe credential is unavailable; no request sent")
    questions = json.loads(json.dumps(context["questions"]))
    for question in questions.values():
        if question["type"] == "bool":
            question["type"] = "noul"
    payload = dict(context, questions=questions, model=MODEL)
    request = Request(ENDPOINT, data=encoded(payload), method="POST",
                      headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
    try:
        with build_opener(NoRedirect).open(request, timeout=30) as response:
            raw = response.read(MAX_RESPONSE + 1)
    except HTTPError as error:
        status = error.code
        error.close()
        raise ValueError(f"TypeSafe HTTP {status}; no automatic retry; response body omitted") from None
    except (URLError, TimeoutError, OSError):
        raise ValueError("TypeSafe transport failure; no automatic retry") from None
    if len(raw) > MAX_RESPONSE:
        raise ValueError("TypeSafe response exceeds limit")
    try:
        return json.loads(raw)
    except (ValueError, UnicodeError):
        raise ValueError("TypeSafe returned invalid JSON") from None


def probability(value):
    if type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1:
        raise ValueError("invalid classifier probability")
    return value


def summarize(request, response):
    if not isinstance(response, dict) or response.get("stopReason", "stop") != "stop":
        raise ValueError("classifier did not complete successfully")
    if not isinstance(response.get("model"), str) or not response["model"]:
        raise ValueError("classifier model identifier missing")
    answers = response.get("answers")
    if not isinstance(answers, dict) or set(answers) != set(request["questions"]):
        raise ValueError("missing or unexpected classifier answers")
    for name, question in request["questions"].items():
        answer = answers[name]
        if not isinstance(answer, dict):
            raise ValueError("invalid classifier answer")
        if question["type"] == "bool":
            if answer.get("type") not in ("bool", "noul"):
                raise ValueError("invalid boolean answer type")
            probability(answer.get("probability") if answer["type"] == "bool" else answer.get("noul"))
            continue
        if answer.get("type") != "choice" or answer.get("choice") not in question["criteria"]:
            raise ValueError("invalid classifier choice")
        distribution = answer.get("probabilities")
        if not isinstance(distribution, dict) or set(distribution) != set(question["criteria"]):
            raise ValueError("classifier distribution does not match criteria")
        values = [probability(value) for value in distribution.values()]
        if abs(sum(values) - 1) > 0.01 or distribution[answer["choice"]] < max(values) - 0.001:
            raise ValueError("invalid classifier distribution")
        probability(answer.get("confidence"))
    checks = request["state"]["checks"]
    verdict = "failed" if any(c["result"] == "fail" for c in checks) else (
        "checks_passed" if checks and all(c["result"] == "pass" for c in checks) else "incomplete")
    subsystem = answers["subsystem"]
    # An uncalibrated review threshold, not a probability that hardware is correct.
    claim = answers["claim_supported"]
    claim_probability = claim.get("probability") if claim["type"] == "bool" else claim["noul"]
    review = (any(answer["confidence"] < 0.8 for answer in answers.values() if answer["type"] == "choice")
              or subsystem["choice"] == "unknown" or answers["evidence_level"]["choice"] == "insufficient"
              or 0.2 < claim_probability < 0.8)
    return {"deterministic_checks": verdict, "advisory_subsystem": subsystem["choice"],
            "review_required": review, "automatic_action": "none", "compatibility_verdict": "not_established"}


def revision():
    try:
        result = subprocess.run(["jj", "log", "-r", "@", "--no-graph", "-T", "commit_id"],
                                cwd=ROOT, capture_output=True, text=True, timeout=5, check=True)
        value = result.stdout.strip()
        return value if re.fullmatch(r"[0-9a-f]{40,64}", value) else None
    except (OSError, subprocess.SubprocessError):
        return None


def evaluate(request, output, response=None, call=remote):
    output = Path(output).resolve()
    if output.is_relative_to(ROOT) and not output.is_relative_to(ROOT / ".pi" / "jev-runs"):
        raise ValueError("evaluation logs must be outside the repository or under ignored .pi/jev-runs/")
    record = {"format_version": 1, "rubric_version": read_json(RUBRIC)["version"],
              "created_utc": datetime.now(timezone.utc).isoformat(), "jj_commit": revision(),
              "transport": "pi-classifier-import" if response is not None else "typesafe-http",
              "requested_model": MODEL, "request": request, "request_sha256": digest(request),
              "rubric_sha256": digest(request["questions"]), "status": "pending"}
    # Reserve the output before calling the API. Refuse overwrite without spending tokens.
    descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        json.dump(record, stream, indent=2)
        stream.flush()
        try:
            result = response if response is not None else call(request)
            summary = summarize(request, result)
            serialized = encoded(result).decode()
            for name in ("TYPESAFEAI_API_KEY", "TYPESAFE_API_KEY"):
                secret = os.environ.get(name)
                if secret and secret in serialized:
                    raise ValueError("credential found in provider response; response omitted")
            record.update(status="completed", response=result, summary=summary)
        except (ValueError, TypeError, KeyError) as error:
            # Do not retain raw provider errors or malformed responses that could echo secrets.
            record.update(status="error", error=str(error))
        stream.seek(0)
        json.dump(record, stream, indent=2, allow_nan=False)
        stream.write("\n")
        stream.truncate()
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path, help="reviewed, minimal text evidence JSON; never a ROM")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--live", action="store_true", help="send one request to TypeSafe; no retries")
    mode.add_argument("--response", type=Path, help="record a Pi models.classify response without another request")
    parser.add_argument("--output", type=Path, help="new evaluation log; required for --live/--response")
    args = parser.parse_args()
    try:
        request = context(read_json(args.evidence))
        if not args.live and args.response is None:
            print(json.dumps(request, indent=2))
            return 0
        if args.output is None:
            raise ValueError("--output is required for evaluation")
        response = read_json(args.response, MAX_RESPONSE) if args.response else None
        if args.response and not isinstance(response, dict):
            raise ValueError("imported response must be an object; no request sent")
        result = evaluate(request, args.output, response)
        print(json.dumps({"log": str(args.output), "status": result["status"],
                          "summary": result.get("summary"), "error": result.get("error")}))
        return 0 if result["status"] == "completed" else 1
    except (OSError, ValueError, TypeError) as error:
        # JSON parser errors omit the original input; no request headers are printed.
        print(f"Jev evaluation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
