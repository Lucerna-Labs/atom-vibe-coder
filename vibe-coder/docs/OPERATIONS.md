# Operations and Recovery

## Build and Static Verification

Run from `vibe-coder/`:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

All Rust crates must remain below 4,000 Rust source lines. Split ownership before
a crate reaches the cap.

## Durable State

The planner writes immutable, sequential ledger snapshots. The scratchpad writes
immutable, hash-linked entries under a build and provider-model scope. Both stores
use process-safe locking and reject tampering, gaps, duplicate sequence numbers,
scope mismatches, and writes after sealing.

After power loss, reopen the coordinator and scratchpad from the same state root.
The current step, retries, completed outputs, deferred debt, wiring status, and
scratchpad chain are reconstructed from disk. The model is not asked to remember
or summarize the missing session.

## Credentials

Persist only the credential environment-variable name. Set the value in the
launching process environment or an approved secret broker. Never place keys in
configuration files, prompts, body templates, logs, screenshots, artifacts,
scratchpads, graph nodes, ledgers, or Git history.

The adapter binds prepared configuration to a hash of endpoint and credential.
Credential rotation requires rebuilding the adapter; a mismatch fails before
HTTP execution.

## Operational Stop Conditions

Do not claim production readiness from compilation, unit tests, a smoke launch,
a proof record, model prose, or a screenshot. A release needs the full verification
contract, a real Qwen3.5 9B Q8-or-stronger thinking-model workflow, multiple
generated applications, inspection of their artifacts, and a live native input
round-trip that changes rendered state.
