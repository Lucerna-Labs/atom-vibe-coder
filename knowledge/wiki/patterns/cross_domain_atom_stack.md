# Cross-domain atom composition — PRE, EXTRACT, QUANTIZE, POST, VERIFY
tags: atom, atoms, atom-stack, atom-composition, cross-domain, pipeline, pre, extract, quantize, post, verify, gate, codec, quantizer, walsh, hadamard, symmetric, max-abs, block, outlier, cosine, kl, divergence, avalanche, hash, integrity, enum, error-handling, exhaustive-match, display, dependency-free, std-only, single-threaded, staged

A reference architecture for a small std-only Rust program that composes stages borrowed from unrelated domains into a bounded pipeline with two independent acceptance gates. The reference implementation is a miniature signal codec (Walsh-Hadamard-style pairwise mix as PRE, top-K outliers as EXTRACT, symmetric Q4 max-abs as QUANTIZE, row-norm preservation as POST, KL + cosine dual gate as VERIFY), but the shape generalises to any staged compression, transformation, or filter chain that borrows atoms from more than one discipline and needs a measurable accept-or-reject decision at the end.

## Structural pattern

- **Five stages, five roles, one atom per role.** PRE transforms (invertible), EXTRACT pulls out a sparse tail, QUANTIZE is the actual coarse map, POST repairs the coarse output, VERIFY decides whether to accept the composed result. The order is doctrine, not opinion.
- **Every atom carries a bounded contract in its signature.** Input shape, output shape, and a matching failure variant on the shared error enum. A stage that could accept invalid input (empty, non-finite, oversized) rejects it explicitly with `Err(...)` so the composition function does not have to guess.
- **Two independent VERIFY gates.** One measure from information theory (symmetric KL over normalised magnitude histograms) and one from vector geometry (cosine similarity). Both must pass; either failing rejects the composed pipeline. Redundant checks catch what one lens misses.
- **`compose_and_gate` is the top-level function.** It calls the atoms in order, threads results forward, and returns either the reconstruction or the `VerifyRejected` variant naming which measurement fell outside its threshold.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a codec, quantizer, lossy compression, staged filter, transformation chain, "run these steps then check the result", or "compose atoms from disciplines A and B and see if it beats the baseline." The specific atoms vary by domain; the five-role composition with two-gate verify stays.

## Anti-patterns to avoid

Do not skip a role because it "does not apply" — use the identity (Null) shape for that role instead so the composition stays uniform. Do not use just one VERIFY gate; two independent gates catch structural drift a single measurement misses. Do not reach for atomic types, locks, or worker threads for a "codec" at this scale — the atom pattern is sequential; parallelism is a separate concern that comes after a correct sequential composition exists.

## Related

[[wiki:atom-quantizer]]
[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/cross_domain_atom_stack.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
