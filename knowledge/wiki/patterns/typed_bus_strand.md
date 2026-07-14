# Typed pub/sub bus with Strand trait, Socket by TypeId, and deterministic tick executor
tags: bus, strand, socket, dispatcher, router, pub-sub, publish-subscribe, typed, type-id, trait, plug-and-play, plugin, message-bus, event, runtime, in-process, sequential, tick, deterministic, dependency-free, std-only, no-async, single-threaded, enum, error-handling, exhaustive-match, display

A reference architecture for a small std-only Rust program that implements the core contract of a plug-and-play message bus: workers declare typed inputs and outputs, the bus wires them by TypeId with no manual wiring code, and a deterministic executor ticks every worker in registration order. The reference implementation is a sequential kernel that routes messages from a Source strand to a Sink strand, but the shape generalises to any plugin runtime, event router, worker pool, or in-process fabric where the connection topology should be discovered from types rather than declared.

## Structural pattern

- **`Socket` = `(name: &'static str, type_id: TypeId)`.** The name is human-readable; the type is the actual address. Two sockets are compatible when both components match. This is what makes the bus wire itself.
- **`Strand` trait declares four things and only four things.** `name`, `inputs -> &[Socket]`, `outputs -> &[Socket]`, `run(&mut self, bus: &mut BusHandle) -> Result<(), BusError>`. A strand knows nothing about other strands.
- **`BusHandle` is what the strand sees.** `publish::<T>(socket, value)`, `recv::<T>(socket) -> Result<Vec<T>>`, `log`. Publish appends to an outbox; recv drains the inbox filtered by socket name plus TypeId equality. A wrong-type recv on a matching socket name is a hard `TypeMismatch` error, not a silent skip.
- **`Bus` kernel owns the strand registry and per-strand inboxes.** `tick()` runs every strand once in registration order, drains its outbox, and fans each envelope to every subscriber whose input socket matches by `(name, TypeId)`. There is no configuration file; the wiring emerges from the strand declarations.
- **`Detach` is a soft signal.** A strand returning `Err(Detach)` has its outbox routed to subscribers first, then it is removed from the registry so it never runs again. In-flight envelopes are never dropped by a detach.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a message bus, plugin runtime, event router, worker fabric, dispatcher, or any system where independent components must connect by declared type without manual wiring. Also correct for a small task orchestrator that runs a set of workers each tick.

## Anti-patterns to avoid

Do not reach for atomics, locks, worker threads, `async`, or `Pin` to implement a "message bus" at this scale. Real production buses are threaded but only because process-wide fan-out and lane crypto demand it — the CORE contract (strands declare types, bus routes by TypeId) is entirely expressible in a sequential loop, and getting that shape right sequentially is the prerequisite for any threaded upgrade. Do not have strands look each other up by name — the whole point of the pattern is that they cannot. Do not swallow a type mismatch on recv; it must be a real error variant.

## Related

[[wiki:spiderweb-bus-reference]]
[[wiki:spiderweb-bus]]
[[wiki:atom-quantizer]]

## Reference implementation

knowledge/wiki/examples/typed_bus_strand.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
