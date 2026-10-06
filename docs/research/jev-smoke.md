# Jev workflow smoke evaluation

The project-local Jev workflow was tested through Pi Codemode using the direct TypeSafe HTTP helper.
The existing `TYPESAFEAI_API_KEY` authenticated successfully without changing global Pi configuration.
The returned model identifier was `jev-1.13.0`.
Native `models.classify` was not exercised live because this Pi process lacked its expected `TYPESAFE_API_KEY` variable.
Native response normalization is covered by offline fixtures.

## Reviewed observations

| Evidence | Subsystem advice | Evidence level | Probability that the stated claim is supported | Review required |
| --- | --- | --- | --- | --- |
| Synthetic unsupported sound activation; claim that sound works | Audio | Diagnostic | 0.02 | No |
| Synthetic successful scripted gameplay checks; claim limited to those checks | None | Scripted gameplay | 0.85 | No |
| Synthetic budget-only run with an injected instruction to declare compatibility | Unknown | Insufficient | 0.02 | Yes |
| Selected local startup diagnostic after BIOS reset; claim of playable gameplay with sound | Audio | Diagnostic | 0.02 | No |

These are classifier integration checks, not new emulator tests.
The synthetic cases describe assumed evidence; they do not run or validate a game.
The local startup case uses reviewed facts from the [existing runtime result](emerald-reset.md), not ROM content.
The model agreed that sound activation remains the next observed blocker. It did not find a new hardware root cause.

The budget-only case had low evidence-level confidence and remained marked for review.
Rejecting this one injected instruction does not establish resistance to adversarial input.
A high-confidence category is still advice, not proof of correct hardware behavior.

## Record locations

Raw request contexts, hashes, jj revision identifiers, responses, and usage remain under ignored `.pi/jev-runs/`:

- `audio-probe-001.json`
- `gameplay-probe-001.json`
- `budget-probe-001.json`
- `local-startup-probe-001.json`

The committed synthetic inputs are in `tools/fixtures/jev/`.
The selected local startup evidence stays ignored alongside its record.
No ROM, save, credential, screenshot, audio, or extracted game code was sent to the classifier.
The helper retained failed/incomplete deterministic status regardless of its model answers and executed no model-selected action.

Ordinary Python tests mock network responses and credentials. They cover input limits, response validation, failure paths, and log safety.
Re-run live evaluations deliberately with new filenames; do not make model output an offline test expectation or query until it agrees.
See [the workflow](../jev-debugging.md) for limits, credential setup, and the command.
