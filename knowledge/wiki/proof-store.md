# Proof Store
tags: proof, store, durable, jsonl, evidence

Native proof runs append JSONL records under the MathAtomsCoder local app data store. Records include recipe, status, atoms, evidence count, route length, blockers, provider execution state, provider model, provider endpoint, provider output length, and provider output hash.

Persistent store writes are fail-closed. A proof may only be learned back into the wiki graph after the JSONL append succeeds and the stored record status is proven; blocked or provider-pending records remain durable audit history but do not become positive RAG evidence. A write failure emits StoreBlocked on the Spiderweb Bus and prevents the UI from claiming a proven run.

[[proof-loop]]
[[provider-model-loop]]
[[wiki-graph-rag]]
