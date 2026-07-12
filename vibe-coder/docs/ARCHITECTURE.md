# Atom Vibe Coder Architecture

## Ownership Boundary

`vibe-coder/` owns the coding harness. `atom-rendering-engine-main/` remains a
consumed component that supplies the native renderer, Spiderweb Bus, Wiki Graph,
hashing, JSON, provider transport, verification, proof, and learning primitives.
The harness is not nested inside the renderer and does not make renderer crates
responsible for build planning.

## Runtime Flow

```text
natural-language request
-> persistent build planner
-> current step skill
-> relationship-ranked Wiki Graph RAG
-> build-and-model scoped scratchpad projection
-> Spiderweb L0 transport
-> Spiderweb L1 typed message
-> Spiderweb L2 flow/intersection
-> Spiderweb L3 orchestration
-> thinking-required provider turn
-> model output artifact
-> deterministic real gate
-> append-only ledger snapshot
-> next step or bounded correction
```

Wiki Graph RAG and the scratchpad are separate mandatory inputs. Graph RAG owns
durable relationship knowledge, recipes, and prior verified evidence. The
scratchpad owns temporary working context for one build and one provider-model
identity. It is not long-term memory and cannot replace graph retrieval.

## Six-Stage Spine

1. Intake records complete requirements without inventing architecture.
2. Blueprint freezes crate ownership, typed message contracts, DAG, build order,
   coupling order, and independent review.
3. Crate Build completes each crate in topological order with warning-clean
   compilation and real unit tests.
4. Crate Couple wires one frozen contract at a time over Spiderweb Bus and proves
   emission plus handling.
5. Build Test runs check, test, Clippy, real user workflows, bus round-trips, and
   independent implementation review.
6. Launch Proof keeps the real app alive at a usable screen, captures startup and
   visual evidence, then proves an input traversed L0-L3 and changed rendered state.

The model has no authority to advance a stage. Only the deterministic planner,
after recomputing on-disk evidence, can mutate build state.

## Runtime Composition

`atom-vibe-runtime` creates an immutable session for each natural-language build,
opens the current planner ledger and provider-model scratchpad, retrieves pinned
graph contracts plus relationship-ranked task evidence, releases only the current
skill, and prepares a stale-safe provider request. Changing the planner revision,
current step, provider, model, endpoint, wire format, or thinking level invalidates
that prepared request before HTTP execution.

Accepted provider output is written as a content-addressed artifact. A hash-chained
turn record binds request, raw response, output, token usage, thinking evidence,
graph node IDs, context route, provider-result route, scratchpad entry, planner
revision, provider, and model. Restart reloads the session, planner, scratchpad,
and turn chain and recomputes every artifact hash.

The native PMRE shell consumes this composition root without moving planner or
provider ownership into renderer primitives. `Run` creates and prepares the
durable build session; `Vibe Step` executes only the currently released skill on
a worker thread, returns ownership of the runtime to the UI, and exposes the
prepared, running, verification-pending, or blocked state in the native title.
The original `Provider` control remains the meticulous product-build route.

## Trust Boundary

Mode policy and the current skill occupy the provider system/instructions role.
Operator text, graph excerpts, scratchpad entries, prior output, tool output, and
failure logs are encoded as untrusted data. Credentials are loaded by environment
name only at the transport boundary and never enter prompts, command arguments,
receipts, graph nodes, scratchpads, or ledgers.

## Failure and Recovery

Each eligible stage failure permits at most six autonomous corrections. Every
correction is written to the scoped scratchpad, retrieves fresh graph context,
and reruns the same real gate. The seventh eligible failure hard-halts. Planner
and scratchpad stores are append-only and hash chained, allowing exact recovery
after interruption without asking the model to reconstruct state from memory.
