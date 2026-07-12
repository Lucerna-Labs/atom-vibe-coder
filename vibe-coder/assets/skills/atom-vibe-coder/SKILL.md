---
name: atom-vibe-coder
description: Autonomous natural-language coding workflow using Wiki Graph RAG, Spiderweb Bus, a scoped model scratchpad, deterministic planning, and real build evidence.
---

# Atom Vibe Coder

The user describes the product in natural language. The controller owns all
internal routing and must not require the user to explain packets, recipes,
message paths, renderer writes, or verification mechanics.

## Runtime Shape

Every build follows this subsystem path:

```text
Natural-language intent
-> Wiki Graph relationship retrieval
-> build-and-model scratchpad projection
-> Spiderweb L0/L1/L2/L3 context route
-> current build-step skill
-> thinking provider packets
-> deterministic artifact gate
-> append-only ledger snapshot
-> next skill or bounded correction
```

The scratchpad is active working state, not long-term memory. It is isolated by
build and provider-model identity, survives interruption, and is supplied beside
Wiki Graph evidence. It never replaces graph retrieval.

## Non-Negotiable Rules

1. The planner stays between model output and state mutation.
2. Only the current step skill is active in full; all other skills are summaries.
3. Thinking is always enabled. A response without reasoning evidence fails.
4. The recommended local minimum is Qwen3.5 9B Q8 or a demonstrably stronger
   thinking model. Lower quantizations cannot qualify release evidence.
5. No placeholder, stub, omitted section, fake output, or deferred completion.
6. Warnings are errors. Check, test, and Clippy run with warnings denied.
7. Smoke tests do not count as functional testing.
8. Proof claims do not count as production confirmation.
9. A screenshot does not count as launch proof without a live bus round-trip and
   visible rendered state change.
10. Provider, graph, scratchpad, bus, gate, correction, and launch activity is
   logged with recomputable artifacts.
11. A bounded correction is always reverified by the same real gate.

## Build Spine

1. `atom-build-intake`
2. `atom-build-blueprint`
3. `atom-crate-build`
4. `atom-crate-couple`
5. `atom-build-test`
6. `atom-launch-proof`

No step can be skipped, merged, or advanced by model assertion.
