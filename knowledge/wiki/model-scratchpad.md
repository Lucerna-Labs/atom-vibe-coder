# Atom Vibe Coder Model Scratchpad
tags: scratchpad, model, working-context, resume, wiki-graph, spiderweb

Status: active

Each build and provider-model identity owns an isolated, bounded, tamper-evident
scratchpad. It stores current observations, decisions, packet outputs, verified
failures, corrections, and checkpoints so a small thinking model can resume
without reconstructing active work from long-term memory.

## Relationship To Wiki Graph RAG

The scratchpad does not replace Wiki Graph RAG. Every build step receives both:
relationship-ranked graph evidence for durable architecture and recipe knowledge,
and a scoped scratchpad projection for active work. The provider sees both as
untrusted data beneath the trusted mode and current-step instructions.

## Isolation

Scratchpads are separated by build ID and hashed provider-model identity. They
are never searched across builds, never exposed as a general memory tool, and
never promoted into graph learning. Completion seals the append-only hash chain.

## Spiderweb Route

Graph evidence enters at the retrieval on-ramp. Scratchpad projection meets it
at the coder context intersection. The combined context moves through L2 flow
and L3 orchestration to the provider. Scope mismatch or missing graph evidence
fails closed before model execution.
