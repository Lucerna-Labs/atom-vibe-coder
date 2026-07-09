# Proof Store
tags: proof, store, durable, jsonl, evidence

Native proof runs append JSONL records under the MathAtomsCoder local app data store. Records include recipe, status, atoms, evidence count, route length, blockers, provider execution state, provider model, provider endpoint, provider output length, and provider output hash.

Persistent store writes are fail-closed. A proof may only be learned back into the wiki graph after the JSONL append succeeds and the stored record is positive evidence: status proven, no blockers, nonzero evidence, at least one full L0-L3 route, and for provider recipes, provider:ran plus model, endpoint, FNV output hash, and positive output length. Blocked, provider-pending, unaudited, or tampered records remain durable audit history but do not become positive RAG evidence. A write failure emits StoreBlocked on the Spiderweb Bus and prevents the UI from claiming a proven run.

Persistent store reads are also fail-closed. Corrupt or tampered JSONL records block startup proof learning through a StoreBlocked Spiderweb route instead of silently dropping bad lines.

[[proof-loop]]
[[provider-model-loop]]
[[wiki-graph-rag]]
