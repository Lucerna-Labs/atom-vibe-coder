# Meticulous Work Packets
tags: provider, work-packets, small-model, proof, resume, spiderweb

Every provider route starts from one operator-level natural-language request and expands automatically. The operator does not need to name renderer APIs, Rust symbols, packet stages, or Spiderweb message paths.

The base sequence is intent normalization, functional requirements, quality requirements, architecture, and a strict file manifest. Each manifest file adds four ordered packets: file contract, implementation, adversarial review, and complete correction. Corrected files are reduced through bounded three-input integration groups before integration, real functional verification planning, final hostile review, and finalization. One file produces 13 packets; larger manifests add four packets per file plus the required hierarchical integration groups.

Planning and review packets must return one exact ID-bound JSON object. File packets must return one complete fenced file, never a patch, omitted section, placeholder, or TODO. Relative paths reject absolute roots, drive prefixes, duplicate normalized paths, empty components, and parent traversal.

Packet prompts contain the original request, canonical atom stack, untrusted graph evidence, and only the bounded dependency outputs required by that packet. Untrusted evidence and prior output always appear before the final output contract. The corrected file packets are the deliverable source of truth. Multi-file delivery is assembled locally, so the model does not regenerate all files in one final context.

Envelope output is redacted before persistence. Source artifacts are never rewritten: credential-bearing source fails closed and ordinary source remains byte-for-byte intact. Every packet is written immutably, flushed, read back, SHA-256 addressed, and bound to the provider model. The schema-v2 expanded manifest records identity hashes, packet IDs, order, stages, contracts, file owners, exact dependencies, and output limits. Verification reconstructs the canonical plan ID and complete packet DAG rather than trusting the manifest description. A missing, malformed, reordered, wrong-model, oversized, noncanonical, or tampered packet fails closed.

Work-plan execution emits `WorkPlanCreated` at L2, one `WorkPacketExecuted` message per completed packet, and `WorkPlanCompleted` at L3 before a `verification pending` record. Model completion cannot capture production proof. The plan lock serializes identical concurrent plans and only its owner may release it. Resume identity includes the provider request contract and a one-way credential-scope hash, so packets cannot cross provider accounts. A repeated identical route resumes verified packets; it does not call the provider again for completed work.

Complex reference recipes are chunked into individual graph nodes: [[wiki:2d-engine-build]], [[wiki:3d-engine-build]], [[wiki:wifi-adapter-build]], and [[wiki:browser-engine-build]]. The browser recipe is explicitly incomplete and non-proven.

[[provider-api]]
[[proof-store]]
[[self-learning]]
[[wiki-graph-rag]]
