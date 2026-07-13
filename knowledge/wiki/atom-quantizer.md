# Cross-Domain Atom Composition (Atom Quantizer reference)
tags: atom, atoms, atom-stack, atom-composition, cross-domain, pipeline, pre, extract, quantize, post, verify, gate, quantizer, quantization, gguf, codec, signal-processing, information-theory, kl-divergence, cosine-similarity, hadamard, walsh, dct, wavelet, haar, lloyd-max, mu-law, error-diffusion, sigma-delta, outlier, sparse, avalanche, hash, integrity, kernel, driver, invariant, contract, bounded, measurable

Atom-lineage reference project at https://github.com/Lucerna-Labs/atom-quantizer — an adaptive Q2/Q4/Q8 weight codec for GGUF language models built by treating mechanisms from unrelated disciplines as bounded, measurable atoms and composing them in a fixed PRE → EXTRACT → QUANTIZE → POST → VERIFY chain. Documented here as a proven-live example of what the atom doctrine looks like when it lands in a nontrivial codebase, so agents building a kernel, driver, codec, pipeline, or any staged mechanism have a concrete reference for how atoms are named, composed, and gated. This is a durable knowledge node, not a source drop; the source is on GitHub.

## The architectural rule an atom must satisfy

Only accept a cross-domain analogy as an atom if it has ALL of:

1. A narrow input/output contract.
2. A reversible or explicitly lossy boundary.
3. A measurable storage and fidelity cost.
4. A legal place in the PRE → EXTRACT → QUANTIZE → POST → VERIFY chain.
5. Evidence that the composed stack beats or clarifies a baseline.

That rule keeps cross-domain thinking from becoming decorative metaphor. An idea earns its place by surviving composition and measurement, not by being interesting.

## The current v0.2.3 composed stack (proven live)

```
rowH8 pre-transform  →  Q2/Q4/Q8 block quantization  →  blind RF repair  →
row-norm preservation  →  KL + cosine dual gate  →  integrity verification
```

Each arrow is a stage. Each stage is an atom borrowed intact from a distinct domain, wrapped in a bounded Rust type with a state_shape annotation and a measurable contract. On the shape-aware synthetic benchmark, `atom 28 refract` (bulk/tail split) landed at 0.139× baseline MSE and `atom 12 compose` (nested-scale) at 0.713× — the composition doctrine survives real measurement.

## The atom taxonomy — how to name and shape yours

Every atom in the reference codec is annotated with:
- Its role: PRE (transform / predict / whiten, invertible), EXTRACT (pull sparse tail, keep at higher precision), QUANTIZE (the actual code map), POST (repair or de-transform), VERIFY (gate).
- Its state_shape: `any` (identity or shape-agnostic), `1D` (block-scoped over the flat vector), `2D` (needs `out_len × in_len` matrix structure).
- Its source domain — one of: fast signal transforms, digital signal processing, linear algebra, information theory, vector geometry, hashing / integrity, telephony / audio coding, image compression, multiresolution signal analysis, classical scalar quantization, sparse representation, ensemble error cancellation.

A stack is well-formed when every stage's shape is compatible with what the prior stage produced. Compose across roles, not within — two PRE stages back-to-back is a smell; PRE then EXTRACT then QUANTIZE then POST then VERIFY is the doctrine.

## Live-vs-research boundary — always mark it

Atom Quantizer's docs draw a hard line between "wired into the current path" and "discovery kit — real source exists, exercised by stack search, not part of every normal encode." Mirror this in any atom-lineage project: do not let a research atom silently claim production authority. In the reference codec, discovery atoms live in `src/wq.rs` and `src/stacks.rs` behind explicit selectors; live atoms are wired through `src/main.rs`.

## Cross-domain atoms actually landed in the reference codec

| Source domain | Atom | Borrowed invariant | Role |
|---|---|---|---|
| Fast signal transforms | Row Walsh-Hadamard `rowH8` | orthonormal, self-inverse, norm-preserving rotation | PRE — spreads outliers before block quantization |
| Digital signal processing | Blind finite receptive-field repair | 5-tap magnitude-aware low-pass reconstruction | POST — reduces local decode noise blind |
| Linear algebra | Preserve row norm | L2 norm restored per row | POST — repairs magnitude after inverse-transform + decode |
| Information theory | Bidirectional KL gate | forward + reverse KL over sampled `softmax(W·x)` | VERIFY — rejects on distribution drift |
| Vector geometry | Cosine floor | angular similarity between reference and reconstructed activations | VERIFY — catches structured drift KL alone missed |
| Hashing / integrity | `mix_u32` avalanche tag | deterministic bit diffusion | VERIFY — detects packed-code corruption without cryptography |

## What agents should transfer from this

- When asked to build a codec, quantizer, kernel, driver, pipeline, scheduler, or any staged mechanism, name your atoms and choose their roles BEFORE writing code. The role decides the shape of the type and the shape of the tests.
- Every atom needs a bounded contract (input, output, state_shape, cost). Anonymous helpers are not atoms.
- Compose across roles in a fixed order. Order matters.
- Add a VERIFY stage even when the immediate task doesn't demand one. Two independent gates (KL + cosine in the reference) catch what one misses.
- Distinguish live-path atoms from discovery-path atoms. Do not let discovery silently claim authority.
- Do not use `AtomicBool` / `Mutex` / worker threads to fake "kernel" complexity at small model scale — see the local exemplar [atom_stack_kernel.rs](examples/atom_stack_kernel.rs) for the plain single-threaded shape that actually converges.

[[wiki:production-app-build]]
[[wiki:spiderweb-bus]]
[[rag:wiki-graph]]
