# Verification Contract

## Evidence Rules

Every referenced artifact is a relative safe path plus a recomputed SHA-256 hash.
Narrative claims have zero gate authority. Missing, modified, absolute, escaped,
or mismatched artifacts fail before stage-specific evaluation.

## Stage Gates

| Stage | Required real evidence |
| --- | --- |
| Intake | complete requirements artifact and no premature architecture |
| Blueprint | valid crate DAG/order, frozen messages, protocol artifact, independent review |
| Crate Build | source artifacts, warning-denied check, real unit tests, no stubs or untracked COUPLE markers |
| Crate Couple | each declared message observed emitted and handled over the bus, no direct side channels |
| Build Test | warning-denied check/test/Clippy, every DoD workflow, bus round-trip, independent review |
| Launch Proof | live panic-free process, usable screen, startup log, screenshot, full L0-L3 input/result/render change, zero debt |

Smoke checks do not satisfy functional cases. A corrected final artifact must run
through the same verification gate again until it passes; correction prose is not
closure.

## Current Automated Coverage

The workspace currently covers protocol validation, tamper rejection, fixed-step
release, retry exhaustion, ledger restart recovery, scratchpad isolation and
redaction, relationship-ranked Wiki Graph context, complete L0-L3 routes, mode
policy, provider body formats, credential isolation, response parsing, and
mandatory thinking evidence.

## Release Acceptance Still Required

The integrated native runtime must additionally complete these real workflows
before any production-ready claim:

1. Run with Qwen3.5 9B Q8 or a stronger thinking model and capture reasoning evidence.
2. Build several materially different applications from ordinary natural language.
3. Compile, test, launch, interact with, and visually inspect every generated app.
4. Interrupt and resume an active build without state reconstruction by the model.
5. Exercise provider switching, graph retrieval, scratchpad isolation, retries,
   hooks, skills, MCP lanes, artifact previews, copy/paste/editing, and scrolling.
6. Run adversarial review, fix every finding, and rerun all affected real gates.
