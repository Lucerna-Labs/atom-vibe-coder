---
name: atom-build-blueprint
description: Step 2. Freeze crate ownership, message contracts, dependency DAG, build order, coupling order, and independent review.
---

# Atom Build Blueprint

Retrieve Rust architecture, Spiderweb Bus, renderer, and relevant recipe nodes
from Wiki Graph RAG. Define focused crates with one responsibility each. Freeze
every bus message type, producer, consumer, and failure behavior. Produce an
acyclic dependency graph, exact topological build order, and exact coupling
order.

An independent reviewer identity must inspect boundaries, contracts, and DAG.
Resolve every finding before pass. Amendments create a new blueprint version;
they never silently rewrite the frozen record.

No implementation begins until the artifact-backed blueprint gate passes.
