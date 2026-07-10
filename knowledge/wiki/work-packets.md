# Meticulous Work Packets
tags: provider, work-packets, small-model, proof, resume, spiderweb

Every provider route starts from one operator-level natural-language request and expands automatically. The operator does not need to name renderer APIs, Rust symbols, packet stages, or Spiderweb message paths.

The base sequence is intent normalization, functional requirements, quality requirements, architecture, and a strict file manifest. Every manifest file then passes through three repair cycles: contract-bound implementation review and correction, integration review and integration-aware correction, then functional plus hostile review and final correction. Bounded three-input integration, closure, and release groups prevent large manifests from overflowing a small model context. One file produces 19 packets; larger manifests add six file-owned packets plus the required bounded group packets.

Planning and review packets must return one exact ID-bound JSON object. File packets must return one complete fenced file, never a patch, omitted section, placeholder, or TODO. Relative paths reject absolute roots, drive prefixes, duplicate normalized paths, empty components, and parent traversal.

Provider requests place the packet identity, objective, acceptance gates, and output contract in a trusted system or instructions role. The original request, graph evidence, and bounded dependency outputs are JSON-encoded into a separate user-data role. Integration and release closure packets reject nonempty risk lists; review packets may report risks only because a later file-correction stage consumes them. Final-correction packets are the sole deliverable source of truth. Multi-file delivery is assembled locally, so the model does not regenerate all files in one final context.

Envelope output is redacted before persistence. Source artifacts are never rewritten: credential-bearing source fails closed and ordinary source remains byte-for-byte intact. Every packet is written immutably, flushed, read back, SHA-256 addressed, and bound to the provider model. The schema-v3 expanded manifest records identity hashes, packet IDs, order, stages, contracts, file owners, exact dependencies, and output limits. Verification reconstructs the canonical plan ID and complete packet DAG rather than trusting the manifest description. A missing, malformed, reordered, wrong-model, oversized, noncanonical, or tampered packet fails closed.

Work-plan execution emits `WorkPlanCreated` at L2, one `WorkPacketExecuted` message per completed packet, and `WorkPlanCompleted` at L3 before a `verification pending` record. Model completion cannot capture production proof. The plan lock serializes identical concurrent plans and only its owner may release it. Resume identity includes the provider request contract and a one-way credential-scope hash, so packets cannot cross provider accounts. A repeated identical route resumes verified packets; it does not call the provider again for completed work.

Complex reference recipes are chunked into individual graph nodes: [[wiki:2d-engine-build]], [[wiki:3d-engine-build]], [[wiki:wifi-adapter-build]], and [[wiki:browser-engine-build]]. The browser recipe is explicitly incomplete and non-proven.

[[provider-api]]
[[proof-store]]
[[self-learning]]
[[wiki-graph-rag]]
