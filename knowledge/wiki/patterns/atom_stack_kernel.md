# Atom-stack kernel driver — scan, hash, project, compare, order
tags: kernel, micro-kernel, medium-kernel, atom, atom-stack, scan, hash, project, compare, order, pipeline, driver, scheduler, staged, enum, error-handling, exhaustive-match, display, dependency-free, std-only, single-threaded

A reference architecture for a small std-only Rust program that composes a program-kernel or scheduler from a fixed sequence of named atoms, each with a bounded contract and a matching failure variant. The reference implementation runs the atom stack scan → hash → project → compare → order over a `&[u8]` state, but the shape generalises to any small kernel driver, task scheduler, or single-threaded orchestrator whose steps have a required order.

## Structural pattern

- **One typed `KernelError` enum, one variant per atom.** `ScanFailed`, `HashFailed`, `ProjectFailed`, `CompareFailed`, `OrderFailed`. Every failure site knows which stage failed without threading a stage label through the return value.
- **Each atom is a `fn` method on the driver struct with a narrow contract.** Inputs are references; outputs are `Result<T, KernelError>` where `T` is the specific shape that stage produces. Signatures encode the stage's role — `scan(&self) -> Result<Vec<u8>, _>`, `hash(&self, data: &[u8]) -> Result<u64, _>`, and so on.
- **The `execute` method is the composition site.** It calls the atoms in doctrinal order, threading each stage's output into the next. If any stage returns an error, `?` short-circuits and the whole run reports which stage failed.
- **Inline `#[cfg(test)]` covers every atom's contract independently.** One test per atom plus one full-pipeline test.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a kernel, driver, scheduler, staged pipeline, orchestrator, or "run these steps in this exact order" workload. The specific atoms vary by domain; the composition shape stays the same.

## Anti-patterns to avoid

**Do not reach for atomic types (AtomicBool, AtomicU64), locks (Mutex, RwLock), or worker-thread spawns for a "kernel" at this size.** Small models (9b in particular) will emit `#[derive(Clone)] struct Core { running: AtomicBool }` which fails E0277 because atomics do not implement Clone; the model then burns every repair round trying to fix a self-inflicted invariant. The shape shown in the reference implementation is deliberately sequential. Parallelism is a separate concern that comes AFTER a correct sequential composition exists.

Do not skip stages to "simplify"; the composition doctrine is what makes the pattern generalise. Do not merge two atoms into one function when their contracts differ; that hides one stage's failure inside another's error variant.

## Related

[[wiki:atom-quantizer]]
[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/atom_stack_kernel.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
