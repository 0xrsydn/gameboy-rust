# Jev-assisted GBA debugging

Use `/jev-gb-debug` in Pi to review bounded diagnostic evidence and compatibility claims.
Jev is an optional classifier, not an emulator, debugger, code generator, or hardware reference.
Rust assertions, ROM suite results, and measured device behavior remain authoritative.
This workflow does not implement audio or make the current Emerald startup diagnostic pass.

## Enable the project command

The project provides:

- `.pi/prompts/jev-gb-debug.md`: the `/jev-gb-debug [focus]` prompt command.
- `.pi/skills/jev-gb-debug/SKILL.md`: the evidence collection, evaluation, and regression workflow.
- `.pi/settings.json`: adds Codemode without replacing the inherited tool list.

Review and trust these project resources in Pi, then run `/reload`.
Use `/jev-gb-debug sound initialization` or `/skill:jev-gb-debug`.
A prompt command asks the coding agent to follow the workflow. It is not a background process or a new executable debugger.

## Credentials and execution

Pi's native `models.classify` interface expects `TYPESAFE_API_KEY` for the direct TypeSafe provider.
The local HTTP helper also accepts `TYPESAFEAI_API_KEY`, with that variable taking precedence when both are set.
No key value is printed, saved, or placed in a command argument.
To use the existing key with Pi's native interface in a future session, set the alias before starting Pi:

```sh
export TYPESAFE_API_KEY="$TYPESAFEAI_API_KEY"
pi
```

A child shell cannot update the already-running Pi process environment.
The workflow therefore uses the direct HTTP helper when native TypeSafe authentication is unavailable.
It does not silently choose OpenRouter or another account/provider.
Native classifier calls contribute usage to Pi's session accounting. Direct HTTP calls record usage locally but bypass Pi's model-cost counter.
Zero or missing reported cost does not imply that a request was free.

## Evidence and evaluation

1. Define the claim and deterministic acceptance criteria.
2. Run a bounded test or ROM diagnostic and inspect its exit status.
3. Prepare minimal text evidence. Review every field before sending it.
4. Ask Jev to classify the next subsystem, the evidence level, and whether the exact claim is supported.
5. Compare the advice with code and tests. Preserve uncertainty and disagreements.
6. Implement original regression tests and a general fix, then rerun the same deterministic checks.

The rubric is versioned in [`tools/jev-gb-rubric.json`](../tools/jev-gb-rubric.json).
Its `unknown` option prevents forced subsystem guesses. Evidence levels distinguish diagnostics, checkpoints, and scripted gameplay.
A budget-only run cannot establish gameplay. Startup frames cannot establish a title screen, sound, or save support.
The model never changes a test result or selects an executable shell command.

The helper accepts exactly these evidence fields:

```json
{
  "case_id": "sound-initialization",
  "observations": ["Sound master enable was rejected because the audio engine is not implemented."],
  "checks": [{
    "name": "sound-start",
    "result": "fail",
    "scope": "Sound activation must finish without an unsupported-device diagnostic."
  }],
  "claim": "Sound works."
}
```

Use check results `pass`, `fail`, or `not_run`. Keep exact numeric comparisons in deterministic code.
Use named observations rather than asking Jev to decode ARM instructions, compare hex colors, or calculate timing.
Current Jev models accept text only. They cannot directly inspect frames, audio, video, or binary ROMs.
The existing suite runner can produce button/pixel assertions, but it does not expose the article's live game-control loop.
Future interactive control must use bounded, validated emulator commands and recorded input schedules.

## Preview, run, and record

Preview without credentials or network access:

```sh
direnv exec . python3 -B tools/jev_gb_debug.py tools/fixtures/jev/audio-blocker.json
```

Run one direct TypeSafe request and save a new local record:

```sh
mkdir -p .pi/jev-runs
direnv exec . python3 -B tools/jev_gb_debug.py tools/fixtures/jev/audio-blocker.json \
  --live --output .pi/jev-runs/audio-review-001.json
```

Choose a new output name for each evaluation. The helper reserves the file before sending a request and refuses overwrite.
A pending record means the process ended before recording a complete result; it is not a pass and must not trigger a blind retry.
Output outside the repository is also accepted. In-repository output is restricted to ignored `.pi/jev-runs/`.
The helper creates record files with owner-only permissions. Parent directories must already exist.

The skill contains the native Codemode `models.classify` example.
It previews the same context, makes one classifier call, then imports the result with `--response PATH.json` without a second request.
Pi represents yes/no questions as `bool`/`probability`; the HTTP API uses `noul`/`noul`.
The helper converts request types for HTTP and validates either response format.
It never reads credentials into the Codemode transcript.

Every complete record includes:

- Reviewed evidence and the exact rubric context, plus SHA-256 hashes and rubric version.
- The local jj commit identifier and a UTC timestamp. Local metadata is not sent to Jev.
- Transport, requested model alias, returned model identifier, typed answers, distributions, and available usage.
- Deterministic check status and an advisory summary. Compatibility remains `not_established` regardless of the model answer.
- Explicit error state when authentication, transport, response validation, or classification fails.

The stored context uses Pi's question schema; the HTTP request differs only by the `bool` to `noul` conversion and model field.
The alias `jev-latest` can change. Retain the returned model identifier when comparing runs.
Do not treat replaying the same prompt as a deterministic test.

## Limits and disclosure

Each command permits at most four live evaluations by default. Each request asks all rubric questions together.
The helper limits evidence to 16 KiB, 12 observations, and 24 checks; responses are bounded separately.
Direct HTTP uses a fixed HTTPS endpoint, refuses redirects, has a 30-second socket timeout, and performs no automatic retries.
Use the skill's 45-second tool timeout as the process bound. An aborted request may still incur usage.

Choice confidence below 0.8, unknown/insufficient evidence, or a yes/no probability between 0.2 and 0.8 requires review.
These thresholds are provisional. They are not calibrated measures of GBA hardware correctness.
Do not repeatedly query until the model agrees with a preferred answer.

Never send ROMs, extracted game code/assets, firmware, save data, screenshots, audio, credentials, or unfiltered logs.
Selected emulator-generated diagnostics and original test summaries are sufficient.
The helper rejects unknown fields, excessive input, common credential markers, and common private paths.
These checks do not detect every secret or private detail. The agent must review the full payload before requesting evaluation.
State and model responses are untrusted data. An injection smoke test does not establish a security boundary.

Raw records stay ignored. Commit only reviewed summaries of useful findings, methods, and original regression tests.
Normal Cargo and Python tests stay offline and do not require Jev credentials.

## Validation

The [reviewed smoke evaluation](research/jev-smoke.md) records live classifier outcomes and their limits.
The HTTP path was exercised live. Native Pi response normalization was tested offline; native authentication needs the environment alias above.

## References

- [TypeSafe documentation index](https://docs.typesafe.ai/llms.txt).
- [API reference](https://docs.typesafe.ai/api.md) and [model capabilities](https://docs.typesafe.ai/models.md).
- [Jev with coding agents](https://docs.typesafe.ai/introduction/coding-agents.md): structured decisions, not code generation.
- [Confidence](https://docs.typesafe.ai/confidence.md) and [Jev 1.13 limitations](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md): arithmetic, indirection, option order, and adversarial content.
- [What is Codemode](https://lucumr.pocoo.org/2026/10/6/codemode/): harness-side orchestration and the bounded game-control example.
- Installed Pi documentation: `docs/codemode.md`, `docs/models.md`, `docs/skills.md`, `docs/prompt-templates.md`, and project-trust/configuration references.
