# Atom Vibe Coder Runtime

This workspace owns the coding harness. It is intentionally outside
`atom-rendering-engine-main` because the renderer is a consumed product
component, not the owner of planning, provider work, build gates, or learning.

The runtime has three mandatory foundations:

1. Wiki Graph RAG grounds every build step with relationship-first evidence.
2. Spiderweb Bus carries every build, packet, gate, correction, and proof route
   through L0 transport, L1 message, L2 flow, and L3 orchestration.
3. Each build and provider-model identity receives an isolated scratchpad for
   resumable working context. Scratchpad data is never queried as long-term
   memory and is never promoted into graph learning.

The fixed build spine is Intake, Blueprint, Crate Build, Crate Couple, Build
Test, and Launch Proof. A model completion cannot advance this state machine;
only independently evaluated gate evidence can.

## Model Baseline

Thinking is mandatory for every model turn. A configured thinking flag is not
enough: the response must include provider-side reasoning tokens, a reasoning
field, or a typed thinking block. Missing evidence fails closed.

The recommended minimum local model is **Qwen3.5 9B Q8 with thinking enabled**,
or a demonstrably stronger thinking model. Lower quantizations are useful for
diagnostics but do not qualify a release. Cloud and custom models are accepted
by capability and real gate results, not by brand name.

## Workspace Crates

| Crate | Ownership |
| --- | --- |
| `atom-vibe-build-protocol` | Fixed steps, gate outcomes, artifacts, and planner events |
| `atom-vibe-build-gates` | Deterministic evaluation of on-disk evidence |
| `atom-vibe-build-planner` | Persistent ledger, retries, restart recovery, and planner bus routes |
| `atom-vibe-context` | Wiki Graph RAG plus scratchpad context over Spiderweb L0-L3 |
| `atom-vibe-mode` | Mode policy and progressive step-skill disclosure |
| `atom-vibe-native-bridge` | Native PMRE session state and asynchronous Vibe-step ownership bridge |
| `atom-vibe-provider` | Thinking-required multi-provider turns using credential-safe transport |
| `atom-vibe-runtime` | Durable sessions, current-skill turns, provider receipts, gate submission, and restart recovery |
| `atom-vibe-scratchpad` | Build-and-model scoped append-only working context |

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Provider and model requirements](docs/PROVIDER_MODELS.md)
- [Operations and recovery](docs/OPERATIONS.md)
- [Verification contract](docs/VERIFICATION.md)
