---
name: atom-build-pipeline
description: Six-stage planner contract for complete Atom Vibe Coder builds.
---

# Atom Build Pipeline

The planner releases exactly one step at a time:

```text
Intake -> Blueprint -> Crate Build -> Crate Couple -> Build Test -> Launch Proof
```

Every step begins with Wiki Graph RAG and the active model scratchpad, travels
through Spiderweb L0-L3, and ends at an artifact-backed gate. `Pass` records an
immutable step output and releases the next skill. `Fail` requests a bounded
correction only for eligible defects. `Deferred` is legal only for an explicitly
tracked Crate Couple wiring and never means pass.

Six correction requests are available per step. Every correction reruns the
same gate. Exhaustion hard-halts the build with evidence.

The ledger persists requirements, versioned blueprints, passed step outputs,
crate and wiring status, deferred debt, COUPLE markers, retry records, and final
launch proof. The latest ledger is reconstructed from an append-only hash chain
after interruption.
