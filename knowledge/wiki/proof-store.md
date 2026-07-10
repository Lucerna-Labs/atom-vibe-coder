# Proof Store
tags: proof, store, durable, jsonl, evidence

Native proof runs append JSONL records under the MathAtomsCoder local app data store. Records include recipe, status, atoms, evidence count, route length, blockers, provider execution state, provider model, provider endpoint, provider output artifact/hash/length, work plan ID, expanded work manifest, and packet count. Concurrent writers are serialized, and each append is flushed, synced, sought, and read back before success is returned.

Persistent store writes are fail-closed. Model completion alone is recorded as `verification pending`, never proven. A proof may only be learned back into the wiki graph after a real product harness passes, the JSONL append succeeds, and the stored record is positive evidence: status proven, no blockers, nonzero evidence, at least one full L0-L3 route, and for provider recipes, provider:ran plus model, endpoint, a content-addressed output artifact, recomputed SHA-256 hash, exact output length, and a verified meticulous work plan of at least 19 packets. The expanded manifest and every packet output must reparse and recompute under the same model. Blocked, provider-pending, verification-pending, missing, unaudited, legacy-checksum, coarse, wrong-model, or tampered records remain durable audit history but do not become positive RAG evidence. A write failure emits StoreBlocked on the Spiderweb Bus and prevents the UI from claiming a proven run.

Persistent store reads are also fail-closed. Corrupt or tampered JSONL records block startup proof learning through a StoreBlocked Spiderweb route instead of silently dropping bad lines.

The proof store and learning ledger have separate authority. `proofs.jsonl` answers whether a route is proven. `learning.jsonl` records what failed, what correction was applied, and which real gate or artifact later passed. A failed learning record may guide a retry but cannot become positive proof evidence.

[[proof-loop]]
[[provider-model-loop]]
[[wiki-graph-rag]]
[[wiki:self-learning]]
[[work-packets]]
