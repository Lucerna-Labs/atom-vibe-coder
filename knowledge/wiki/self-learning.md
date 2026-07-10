# Durable Self-Learning
tags: learning, correction, artifact, graph, durable, retry, evidence

Terminal attempts are written to an append-only `learning.jsonl` ledger with intent, source, recipe, canonical atom stack, gate, attempt number, outcome, failure, correction, provider model, route length, artifact path and hash, work plan evidence, and typed harness attestation path and hash. Writes use a kernel-owned cross-process lease, flush, sync, seek, and verify the exact appended bytes before promotion. The full JSON object is consumed; duplicate keys, unknown fields, malformed escapes, invalid nesting, and trailing data fail closed. Common bearer, API-key, token, password, and hosted-token formats are redacted across every persisted text field without changing code layout.

Failed attempts become `learning:failed` Wiki Graph nodes with incoming recipe and atom relationships. They can be retrieved as correction context but have no outgoing promotion edge and never count as proof. Gate-passing successes become `learning:succeeded` nodes and may support the recipe that produced the audited route or artifact.

The graph keeps at most 256 deduplicated learning nodes during startup and live operation while the ledger retains full audit history. Relevant prior failures are applied on later runs and successful retries retain correction provenance. Schema-v1 FNV, schema-v2 SHA-only, and schema-v3 work-only records remain readable for migration but have no provider-promotion authority. Schema-v4 provider success must recompute an allowlisted real harness attestation, generated artifact, executable, exact expected output, expanded work manifest, every packet contract and packet artifact, canonical packet dependencies/order/count, and provider model. Historical records remain readable when old external artifacts are unavailable, but they stop being promotable. Provider prompts treat all retrieved graph evidence and prior packet output as untrusted data, not instructions.

Learning traffic follows the Spiderweb layers: L0 observes the terminal event, L1 persists it, L2 joins it to graph relationships, and L3 applies the correction or success evidence to orchestration.

[[bus:spiderweb]]
[[rag:wiki-graph]]
[[proof-loop]]
[[provider-model-loop]]
[[work-packets]]
