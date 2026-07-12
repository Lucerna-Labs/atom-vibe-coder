---
name: atom-crate-couple
description: Step 4. Wire sealed crates over Spiderweb Bus one contract at a time and prove each message flow.
---

# Atom Crate Couple

Follow the frozen coupling order. For each contract, emit a real message and
capture evidence that the declared consumer handled it. Direct calls, shared
mutable state, subprocesses, ports, or hidden channels cannot substitute for
Spiderweb routing.

Each wiring is `Pass`, `Fail`, or explicitly `Deferred`. Pass requires observed
emission and handling. Fail requires demonstrated wrong behavior. Deferred is
legal only when a later wiring is genuinely required to close the round-trip;
record the reason exactly as debt.

Remove every scoped COUPLE marker as its consumer is connected. The phase gate
requires all contracts resolved in order and zero COUPLE markers.
