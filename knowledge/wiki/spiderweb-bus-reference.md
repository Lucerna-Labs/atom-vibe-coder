# Spiderweb Bus (reference implementation)
tags: spiderweb, bus, strand, socket, kernel, spider, orchestrator, fabric, thread, intersection, vibration, backpressure, highway, preload, lane, pub-sub, publish-subscribe, typed, type-id, plug-and-play, plugin, message-bus, dispatcher, router, runtime, in-process, std-only, dependency-free, no-async, no-tokio, forbid-unsafe, ordo, crypto, chacha20-poly1305, hmac-sha256, p2p, nat, edge, transport, capability

Atom-lineage reference project at https://github.com/Lucerna-Labs/spiderweb-bus — a layered, organic in-process message bus for Rust. No Tokio, no async, no `Pin`, no `Send` futures. Just threads, channels, and types. The kernel is `std`-only, zero external dependencies, and `#![forbid(unsafe_code)]`. This is a durable knowledge node describing the reference bus's structural pattern so agents building any message router, plugin runtime, worker pool, or event fabric have a concrete Rust shape to imitate. The source is on GitHub.

## Distinct from the local wiki's [[spiderweb-bus]] node

The existing [`spiderweb-bus.md`](spiderweb-bus.md) describes the FOUR-LAYER envelope discipline (L0 transport, L1 message, L2 flow, L3 orchestration) that the Atom Vibe Coder's INTERNAL proof loop uses. The reference implementation described here is the general-purpose bus that inspired that layering: strands + sockets + a kernel + a spider. Both are correct; they describe different scales of the same pattern.

## The metaphor an agent can transfer

- **Strands** carry vibrations. Every vibration is a typed message on a strand the kernel manages.
- A **spider** sits in the center, feels every vibration, and knows whether it is benign, threatening, broken, or idle.
- The web is decentralized: every strand is a self-contained crate. The spider is centralized: it sees everything.
- Mechanism lives in the kernel; ALL policy lives in the spider. The kernel is dumb on purpose; make it stay that way.

## The core types

### `Strand` trait — the unit

```rust
pub trait Strand: Send + 'static {
    fn name(&self)    -> &str;
    fn inputs(&self)  -> &[Socket];
    fn outputs(&self) -> &[Socket];
    fn run(&mut self, bus: &mut BusHandle) -> Result<(), StrandError>;
}
```

A strand does not know about other strands, the spider, or the topology. It only knows its `BusHandle` and the types it speaks. Declaring its `inputs()` and `outputs()` as `&[Socket]` lets the kernel wire it into the type registry at register time without any manual wiring code.

### `Socket` — the contract

```rust
pub struct Socket {
    name: &'static str,
    type_id: TypeId,
}
```

Two sockets are compatible when their `TypeId`s match. The name is human-readable; the type is the address. This is the whole reason no wiring is needed.

### `BusHandle` — the transport

```rust
impl BusHandle {
    pub fn publish<T: 'static + Clone + Send>(&mut self, socket: &'static str, value: T) -> Result<(), BusError>;
    pub fn recv<T: 'static + Send>(&mut self, socket: &'static str) -> Result<Vec<T>, BusError>;
    pub fn log(&self, msg: &str);
    pub fn sleep(&self, d: Duration);
}
```

Publish fans out to every subscriber whose declared input Socket matches by `(name, TypeId)`. Recv drains the strand's inbox for that socket.

### The kernel

A small set of threads and a registry. No async. What it owns:

- A type registry: `HashMap<TypeId, Vec<Subscriber>>` for publishers and subscribers.
- Per-strand inbox: `mpsc::Sender<Envelope>` from the kernel to each strand's worker thread.
- Slot table: `name -> factory + done channel + state`.
- Control inbox: kernel listens for `ControlMsg` on `sys.control`.
- Clone registry: `HashMap<TypeId, CloneFn>` for fan-out.

## The layered composition

Ground level is the comfy bus (typed pub/sub). Above it, an optional **highway** lifts a batch into parallel lanes and drops the results back down. The **fabric** — threads and intersections — forms from real traffic. It is observed, never declared. **Vibrations** carry cross-layer signals (backpressure, starvation). The **spider** sits in the middle and reacts. Add a `Spider` and crashed strands get restarted; add a `Highway` and a batch fans across parallel lanes; add a `Preload` and a background reader emits starvation vibrations.

## Lanes — talking to the outside world

Every lane is the same shape: an **ingress** strand (external -> decode -> publish) plus an **egress** strand (subscribe -> encode -> send). Install one with `bus.add_lane("name", &lane)`. The reference repo ships focused lane crates for TCP, P2P mesh, NAT relay, SSH, HTTP, framing, crypto, and the frozen Ordo profile. The Ordo profile fails closed until a process-wide ChaCha20-Poly1305 skin has been installed; wrong-lane, wrong-key, tampered, and replayed envelopes are rejected. That is the actual production posture, not a promise.

## The architectural rule an agent should transfer

- Strands declare inputs and outputs as typed sockets; the kernel wires them by `TypeId` — no manual wiring code, no strand knows another strand's name or interface beyond the type.
- The kernel is deliberately dumb. All policy lives in a separate spider strand that reads system messages and issues control messages back.
- The fabric — threads through the web, intersections where they cross — is OBSERVED from real traffic. Never declared. If you find yourself declaring topology, you are writing the wrong system.
- Backpressure and starvation propagate as first-class vibrations, not as return-code holes.
- No async. Threads + channels + types are enough for an in-process bus. Reach for async only when the boundary is genuinely I/O-bound and cross-process.
- Every external boundary is a lane with the SAME shape (ingress strand + egress strand). Consistency at the perimeter is what lets the kernel stay simple in the middle.
- Do not conflate a plug-and-play kernel with an orchestrator. Mechanism in the kernel, policy in the spider. Keep them separate crates so a wrong-policy commit cannot break the kernel.

## What to read from the repo

- `crates/spiderweb/src/lib.rs` — the kernel: typed pub/sub, fan-out, fabric, lifecycle, `BusStats` telemetry.
- `crates/spider/src/lib.rs` — the orchestrator strand: restart/quarantine policy over `sys.lifecycle` / `sys.control`.
- `crates/highway/src/lib.rs` — layer-2 on-ramp/off-ramp around a parallel lane pool; emits backpressure vibrations.
- `crates/preload/src/lib.rs` — background read-ahead source; emits starvation vibrations.
- `crates/spiderweb-lane-*` — every focused external protocol lane, each the same ingress + egress shape.
- `PERMANENT-LANES.md` and `PRODUCTION-READINESS.md` — the frozen lane profile and the actual production posture, both source-backed.

[[spiderweb-bus]]
[[atom-quantizer]]
[[atoms-hard-rules]]
[[wiki:examples/typed_bus_strand]]
[[wiki:examples/atom_stack_kernel]]
[[wiki:examples/cross_domain_atom_stack]]
[[rag:wiki-graph]]
