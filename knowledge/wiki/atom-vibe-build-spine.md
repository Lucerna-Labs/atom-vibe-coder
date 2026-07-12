# Atom Vibe Coder Build Spine
tags: atom-vibe-coder, build, planner, wiki-graph, rag, spiderweb, scratchpad, evidence

Status: active

Atom Vibe Coder turns a natural-language product request into a complete build
through six deterministic stages. Every stage retrieves relationship-ranked
Wiki Graph evidence, combines it with a build-and-model scoped scratchpad, and
travels over Spiderweb Bus before a model receives the current contract.

## Intake

Record purpose, user behaviors, UI, persistence, external boundaries, execution
siting, exclusions, and definition of done. Do not invent architecture.

## Blueprint

Freeze crate responsibilities, message contracts, dependency DAG, topological
build order, coupling order, and independent review before implementation.

## Crate Build

Build each crate completely in topological order. Reject stubs and placeholders.
Run warning-denied check and real unit tests. Track only scoped COUPLE debt.

## Crate Couple

Wire one frozen message contract at a time over Spiderweb Bus. Pass only after a
real message is emitted and handled. Track genuinely deferred round-trips and
clear all COUPLE markers.

## Build Test

Clear deferred debt. Run warning-denied check, test, and Clippy. Exercise every
definition-of-done requirement through real workflows and bus round-trips.
Smoke tests and proof narratives cannot pass.

## Launch Proof

Launch the operator-facing product, capture startup output and a screenshot,
then drive a real input through L0-L3 and prove rendered state changed. Reconfirm
definition of done and zero debt before completion.

## Correction Loop

Eligible failures receive at most six corrections. Every correction is written
to the scoped scratchpad and re-enters Wiki Graph retrieval with current failure
evidence. The same real gate must pass; a corrected model response is not enough.
