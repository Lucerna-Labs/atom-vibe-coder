# Extending the Atom Set: 8 → 16

**Purpose:** Document the extension from 8 root atoms to 16, the proposed new atoms (9-16), and where they might charge in real work. This is an **exploration document** — verdicts pending.

**Audience:** Rekonquest + AI coders using the Constitution.

---

## 0. Why extend?

The original 8 atoms (`scan`, `hash`, `fold`, `project`, `scale`, `compare`, `combine`, `order`) work well for **finite, deterministic computing**. But when we look at physics — time, space, quantum theory — we see atoms that don't fit cleanly into the 8:

- **Change of basis** (Fourier, dual spaces, position ↔ momentum)
- **Propagation along paths** (fields, flows, currents)
- **Conservation** (what doesn't change across a transformation)
- **Hierarchy** (nested structure, multi-scale systems)
- **Complementary variables** (paired observables)
- **Measurement as a system-changing operation** (vs. passive observation)
- **Symmetry as a first-class structure**
- **Interference** (superposition that isn't just blending)

If any of these atoms charges a real currency in our work, it's worth adopting. If they're painted, they're map data.

---

## 1. The proposed 16

| # | Atom | What it does | Original 8? |
|---|------|--------------|--------------|
| 1 | `scan` | visit each element | yes |
| 2 | `hash` | reduce to identity | yes |
| 3 | `fold` | reduce to one value | yes |
| 4 | `project` | take a slice/view | yes |
| 5 | `scale` | resize/quantize/normalize | yes |
| 6 | `compare` | decide equality/ordering | yes |
| 7 | `combine` | join two things | yes |
| 8 | `order` | establish sequence | yes |
| 9 | `transform` | change of basis/representation | **new** |
| 10 | `flow` | move value along a path | **new** |
| 11 | `preserve` | conservation / invariance | **new** |
| 12 | `compose` | nested structure | **new** |
| 13 | `dual` | complementary / paired | **new** |
| 14 | `measure` | extract observable (system-modifying) | **new** |
| 15 | `symmetrize` | enforce / exploit invariance | **new** |
| 16 | `superpose` | weighted blend with interference | **new** |

---

## 2. Where each new atom might charge

This is **hypothesis territory**. Verdicts come from the bench.

### 9. `transform` — change of basis/representation

**What it might be in our domains:**
- Graphics: rotation, projection (camera)
- Inference: precision changes (FP32 ↔ FP8 ↔ INT4), basis changes in attention
- Bus: serialization format changes
- Quantization: block scale = local coordinate transform

**Painted-fence risk:** low. Transforms are concrete and verifiable.

**Test idea:** Does formalizing quantization's block scaling as a `transform` atom simplify the codec? Does attention head projection compose cleanly as `transform ∘ fold`?

### 10. `flow` — move value along a path

**What it might be in our domains:**
- Bus: message propagation through lanes
- Graphics: ray marching (light flows along rays)
- Inference: token propagation through layers
- Networking: packet flow

**Painted-fence risk:** low — flows are well-defined.

**Test idea:** Does the Spiderweb bus's message propagation match `flow` semantics? Can we describe a unified flow interface across all our transports?

### 11. `preserve` — conservation / invariance

**What it might be in our domains:**
- Bus: bandwidth budgets that don't change
- Inference: KV cache size invariants across layers
- Graphics: total light energy through a closed surface
- Crypto: hash integrity (preserved bit-exactness)

**Painted-fence risk:** medium. "Conservation" is a strong claim that might overstate what's actually happening.

**Test idea:** What invariants does each subsystem actually preserve? List them. Are some of them expressible as `preserve` atoms?

### 12. `compose` — nested structure

**What it might be in our domains:**
- Orchestrator composing atoms into higher-level operations
- Browser: DOM nesting
- Inference: transformer block (multi-head attention composed with FFN)
- Crypto: cipher = compose(permute, substitute)

**Painted-fence risk:** low — composition is already implicit in our work.

**Test idea:** Make `compose` first-class in the orchestrator's API. See if it surfaces cleaner abstractions.

### 13. `dual` — complementary / paired

**What it might be in our domains:**
- Time ↔ frequency (RF repair already uses this)
- Position ↔ momentum (Heisenberg-flavored; don't over-claim)
- Encode ↔ decode (codec duality)
- Forward ↔ backward (autograd, prefix-cache reuse)

**Painted-fence risk:** medium-high. The metaphor is seductive but the math doesn't always carry.

**Test idea:** Where does the time-frequency duality in RF repair actually pay? Where is it painted?

### 14. `measure` — extract observable (system-modifying)

**What it might be in our domains:**
- Telemetry: reading GPU state changes the caching/buffering
- Logging: instrumentation has cost
- Profiling: profilers perturb what they measure
- Crypto: MAC verification reads the message

**Painted-fence risk:** low. Heisenberg-style observation effects are real in computing too.

**Test idea:** Does naming `measure` as distinct from `scan` help us reason about observability cost?

### 15. `symmetrize` — enforce / exploit invariance

**What it might be in our domains:**
- Graphics: rotational symmetry in scene optimization
- Inference: tensor parallelism with symmetric worker roles
- Bus: lane symmetry (HTTP lane and API lane are interchangeable for some uses)
- Crypto: symmetric-key primitives

**Painted-fence risk:** low. Symmetry is concrete.

**Test idea:** Where does exploiting symmetry in MM3E scenes save work? In ds4's distributed inference?

### 16. `superpose` — weighted blend with interference

**What it might be in our domains:**
- Inference: attention = superposition with learned weights
- Graphics: alpha blending is *not* superposition (no interference)
- RF: wave interference (real superposition)
- Quantization: weighted averaging of candidate reconstructions

**Painted-fence risk:** high. Most "blending" in computing is not actually superpose — it's combine with weights. The interference property is special.

**Test idea:** Is attention actually superpose, or just weighted combine? Where does the interference matter?

---

## 3. Cross-domain quick map

| Atom | Time | Space | Quantum | Our work |
|------|------|-------|---------|----------|
| `transform` | Fourier ↔ time | rotate / project | basis change | precision / basis |
| `flow` | propagate | field lines | probability current | bus / layer / ray |
| `preserve` | energy/momentum conserved | flux through closed surface | unitarity | invariants |
| `compose` | multi-scale time | nested regions | multi-particle | orchestrator nesting |
| `dual` | time ↔ frequency | position ↔ momentum | Hilbert dual | encode/decode |
| `measure` | read clock | measure extent | collapse | telemetry |
| `symmetrize` | time-translation invariance | rotation invariance | bosons/fermions | scene symmetry |
| `superpose` | wave interference | field superpose | coherent superpose | attention? |

---

## 4. How to test these (the bench)

For each new atom:

1. **Name the currency** — what would "this atom works" look like?
2. **Find a concrete test** — a project, a kernel, a small experiment
3. **Run the test with and without the atom**
4. **Measure the difference on the named currency**
5. **Record the verdict** (REAL / PAINTED / HURTS / BASELINE)

### Possible test projects

- **MM3E scene with rotational symmetry** → test `symmetrize`
- **Spiderweb bus bandwidth budget** → test `preserve`
- **Ornith Quant asymmetric codec** → already has implicit `transform`; formalize it
- **RF repair in quantization** → test if `dual` formalization helps the bench
- **ds4 telemetry pipeline** → test if `measure` clarifies the cost model
- **Driver Observatory (planned)** → test `flow` for kernel propagation

---

## 5. The bench protocol (for AI coders)

When AI coders are asked to implement any of atoms 9-16:

```
Rekonquest constitution applies.
Three layers: ATOMS → ORCHESTRATOR → SEALED ENGINE.
8 root atoms: scan, hash, fold, project, scale, compare, combine, order.
8 extended atoms (proposed): transform, flow, preserve, compose, dual, 
measure, symmetrize, superpose.
The extended atoms are UNDER TEST. Bench before adopting.
Every adoption must charge a named currency on real data.
PAINTED is a verdict, not a failure. Document it.
```

### Forbidden patterns for extended atoms

- ❌ Adopting any of 9-16 without a bench result
- ❌ Substituting "conventional patterns" for the new atoms (e.g., always using `combine` when `superpose` is right)
- ❌ Renaming atoms 9-16 to "more conventional" terms
- ❌ Pretending the extension is complete when it isn't

---

## 6. Verdict log

This section tracks bench results. Start empty; fill as atoms get tested.

| Atom | Test project | Currency | Verdict | Notes |
|------|--------------|----------|---------|-------|
| — | — | — | — | (empty — first test pending) |

---

## 7. The honest caveats

**The 8 new atoms are hypotheses, not findings.** Some will charge. Some will be painted. That's the doctrine.

**Don't replace the 8 with 16.** The 8 are stable. The 8 are added on top.

**Cross-domain scouting still wins.** If a new atom doesn't apply in any of our domains, it's painted regardless of how elegant it sounds.

**Pattern recognition still matters.** Some atoms will "click" before any bench confirms them. That's expected. Bench confirms or refutes; intuition scouts.

---

## 8. The single line

> *The 8 root atoms travel everywhere. The next 8 are on test. Some will charge; some will be painted. The map gets sharper either way.*

---

*Last updated: 2026-07-06*
*Status: exploration. Verdicts pending bench results.*