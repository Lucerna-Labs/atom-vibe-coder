---
name: atom-build-test
description: Step 5. Run exhaustive warning-clean tests, real user workflows, bus round-trips, and independent implementation review.
---

# Atom Build Test

Clear every deferred wiring first. Run real `cargo check`, `cargo test`, and
`cargo clippy` with warnings denied and captured output. Exercise every
definition-of-done condition through a realistic workflow, including persisted
state and at least one complete bus round-trip.

Smoke checks, process-start checks, model assertions, and proof records are not
functional evidence. Each requirement needs captured output or visual/runtime
artifacts from the real workflow.

An independent reviewer identity inspects the coupled implementation and its
evidence. Resolve every finding, rerun affected tests, and pass the gate again.
