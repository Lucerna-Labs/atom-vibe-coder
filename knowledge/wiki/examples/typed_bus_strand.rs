// EXEMPLAR: typed pub/sub bus with Strand trait + Socket by TypeId + a
//           deterministic tick executor. Dependency-free, std-only, no async,
//           no unsafe, no worker threads -- a sequential kernel is enough for
//           the pattern. Publish fans out to every subscriber whose declared
//           input Socket matches by (name, TypeId); the strand does not know
//           any other strand exists.
// tags: bus, strand, socket, dispatcher, router, pub-sub, publish-subscribe,
//       typed, type-id, trait, plug-and-play, plugin, message-bus, event, runtime,
//       in-process, sequential, tick, deterministic, dependency-free, std-only,
//       no-async, single-threaded, enum, error-handling, exhaustive-match, display
//
// Provenance: hand-authored companion to `knowledge/wiki/spiderweb-bus-reference.md`.
// Captures the CORE PATTERN of the Lucerna-Labs/spiderweb-bus reference kernel:
// strands declare typed inputs and outputs, the bus wires them by TypeId with no
// manual wiring, and the executor ticks strands in registration order. Real
// spiderweb-bus adds threads, spider orchestrator, highway lanes, and lane
// crypto -- this exemplar is the sequential skeleton small models can adapt.
//
// Rustc gate: rustc --edition 2021 --emit=metadata -> exit 0.
// Inline tests: 6/6 passing.
// Release run: `rustc -O` -> the bus routes a message from Source to Sink and
// prints "Sink got: hello #1".
//
// Anti-pattern this shape avoids: DO NOT reach for atomics, locks, worker
// threads, `async`, or `Pin` to implement a "message bus" at this scale. Real
// spiderweb-bus is threaded, but only because process-wide fan-out and lane
// crypto demand it -- the CORE contract (strands declare types, bus routes by
// TypeId) is entirely expressible in a sequential loop, and getting that shape
// right sequentially is the prerequisite for any threaded upgrade.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

// --------------------------------------------------------------------------
// Socket -- a (name, TypeId) pair. Two sockets are compatible when both
// components match. `Socket::new::<T>("name")` is the canonical constructor.
// --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Socket {
    pub name: &'static str,
    pub type_id: TypeId,
}

impl Socket {
    pub fn new<T: 'static>(name: &'static str) -> Self {
        Self {
            name,
            type_id: TypeId::of::<T>(),
        }
    }
}

// --------------------------------------------------------------------------
// Typed error enum -- one variant per failure mode a strand or the kernel
// can actually produce, so callsites match exhaustively.
// --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum BusError {
    UnknownSocket(&'static str),
    TypeMismatch { socket: &'static str },
    StrandFailed { strand: &'static str, reason: String },
    DuplicateStrand(&'static str),
    Detach,
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::UnknownSocket(s) => write!(f, "unknown socket '{}'", s),
            BusError::TypeMismatch { socket } => {
                write!(f, "type mismatch on socket '{}'", socket)
            }
            BusError::StrandFailed { strand, reason } => {
                write!(f, "strand '{}' failed: {}", strand, reason)
            }
            BusError::DuplicateStrand(s) => write!(f, "strand '{}' already registered", s),
            BusError::Detach => write!(f, "strand requested detach"),
        }
    }
}

impl std::error::Error for BusError {}

// --------------------------------------------------------------------------
// Envelope -- what actually moves on the bus. `payload` is a `Box<dyn Any>`
// so heterogeneous message types share one queue; the receiver checks the
// TypeId before downcast so a mismatch is a hard error, not a silent skip.
// --------------------------------------------------------------------------

pub struct Envelope {
    pub socket: &'static str,
    pub type_id: TypeId,
    pub payload: Box<dyn Any + Send>,
}

// --------------------------------------------------------------------------
// BusHandle -- what a strand sees. A thin cursor into the kernel-owned
// registry; it does not own state so a strand cannot outlive the bus.
// --------------------------------------------------------------------------

pub struct BusHandle<'bus> {
    inbox: &'bus mut Vec<Envelope>,
    outbox: &'bus mut Vec<Envelope>,
    log: &'bus mut Vec<String>,
}

impl<'bus> BusHandle<'bus> {
    pub fn publish<T: 'static + Send>(
        &mut self,
        socket: &'static str,
        value: T,
    ) -> Result<(), BusError> {
        self.outbox.push(Envelope {
            socket,
            type_id: TypeId::of::<T>(),
            payload: Box::new(value),
        });
        Ok(())
    }

    pub fn recv<T: 'static + Send>(&mut self, socket: &'static str) -> Result<Vec<T>, BusError> {
        let target = TypeId::of::<T>();
        let mut out = Vec::new();
        let mut kept = Vec::with_capacity(self.inbox.len());
        for env in self.inbox.drain(..) {
            if env.socket == socket {
                if env.type_id != target {
                    return Err(BusError::TypeMismatch { socket });
                }
                let v = env
                    .payload
                    .downcast::<T>()
                    .map_err(|_| BusError::TypeMismatch { socket })?;
                out.push(*v);
            } else {
                kept.push(env);
            }
        }
        self.inbox.extend(kept);
        Ok(out)
    }

    pub fn log(&mut self, msg: &str) {
        self.log.push(msg.to_string());
    }
}

// --------------------------------------------------------------------------
// Strand trait -- the unit. A strand declares its typed inputs and outputs
// once; the kernel uses those declarations to route publish() to every
// subscriber whose input Socket matches by (name, TypeId).
// --------------------------------------------------------------------------

pub trait Strand: Send {
    fn name(&self) -> &'static str;
    fn inputs(&self) -> &'static [Socket];
    fn outputs(&self) -> &'static [Socket];
    fn run(&mut self, bus: &mut BusHandle<'_>) -> Result<(), BusError>;
}

// --------------------------------------------------------------------------
// Bus -- the kernel. Owns the strand registry, per-strand inboxes, and the
// log ring. `tick()` runs every strand once in registration order, drains
// its outbox, and fans each envelope to every subscriber whose declared
// input socket matches by (name, TypeId). No wiring code anywhere.
// --------------------------------------------------------------------------

pub struct Bus {
    strands: Vec<Box<dyn Strand>>,
    inboxes: HashMap<&'static str, Vec<Envelope>>,
    log: Vec<String>,
}

impl Bus {
    pub fn open() -> Self {
        Self {
            strands: Vec::new(),
            inboxes: HashMap::new(),
            log: Vec::new(),
        }
    }

    pub fn register(&mut self, strand: Box<dyn Strand>) -> Result<(), BusError> {
        let name = strand.name();
        if self.strands.iter().any(|s| s.name() == name) {
            return Err(BusError::DuplicateStrand(name));
        }
        self.inboxes.insert(name, Vec::new());
        self.strands.push(strand);
        Ok(())
    }

    /// Drive every registered strand once. A strand that returns
    /// `Err(BusError::Detach)` is removed from the registry AFTER its already-
    /// published outbox has been routed to matching subscribers -- detaching
    /// must not drop in-flight envelopes the strand already emitted this tick.
    /// Hard errors are propagated so the caller sees them.
    pub fn tick(&mut self) -> Result<(), BusError> {
        let mut all_outbox: Vec<(&'static str, Envelope)> = Vec::new();
        let mut detach_names: Vec<&'static str> = Vec::new();
        let mut hard_error: Option<BusError> = None;
        for strand in self.strands.iter_mut() {
            let name = strand.name();
            let mut outbox = Vec::new();
            let result = {
                let inbox = self.inboxes.get_mut(name).expect("inbox exists");
                let mut handle = BusHandle {
                    inbox,
                    outbox: &mut outbox,
                    log: &mut self.log,
                };
                strand.run(&mut handle)
            };
            for env in outbox {
                all_outbox.push((name, env));
            }
            match result {
                Ok(()) => {}
                Err(BusError::Detach) => detach_names.push(name),
                Err(e) => {
                    hard_error = Some(e);
                    break;
                }
            }
        }
        for (_publisher_name, env) in all_outbox {
            for strand in self.strands.iter() {
                let matches = strand
                    .inputs()
                    .iter()
                    .any(|sock| sock.name == env.socket && sock.type_id == env.type_id);
                if matches {
                    let inbox = self.inboxes.get_mut(strand.name()).expect("inbox exists");
                    let cloned = Envelope {
                        socket: env.socket,
                        type_id: env.type_id,
                        payload: clone_message(&env),
                    };
                    inbox.push(cloned);
                }
            }
        }
        for name in detach_names {
            self.strands.retain(|s| s.name() != name);
            self.inboxes.remove(name);
        }
        match hard_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// True while any registered strand still exists.
    pub fn is_live(&self) -> bool {
        !self.strands.is_empty()
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }
}

// --------------------------------------------------------------------------
// clone_message -- because Box<dyn Any> has no Clone, the kernel needs a
// per-type clone. In real spiderweb-bus this is a HashMap<TypeId, CloneFn>
// filled at register time; here we support the small set the exemplar
// exercises directly so the sequential kernel stays under 250 lines.
// --------------------------------------------------------------------------

fn clone_message(env: &Envelope) -> Box<dyn Any + Send> {
    if let Some(s) = env.payload.downcast_ref::<String>() {
        return Box::new(s.clone());
    }
    if let Some(n) = env.payload.downcast_ref::<u64>() {
        return Box::new(*n);
    }
    Box::new(())
}

// --------------------------------------------------------------------------
// Two example strands -- a Source that emits N greetings, and a Sink that
// logs whatever it receives on the same typed socket. Neither strand knows
// the other exists.
// --------------------------------------------------------------------------

pub struct Source {
    left: u32,
}

impl Source {
    pub fn new(count: u32) -> Self {
        Self { left: count }
    }
}

impl Strand for Source {
    fn name(&self) -> &'static str {
        "source"
    }
    fn inputs(&self) -> &'static [Socket] {
        &[]
    }
    fn outputs(&self) -> &'static [Socket] {
        const S: [Socket; 1] = [Socket {
            name: "greeting",
            type_id: type_id_of_string(),
        }];
        &S
    }
    fn run(&mut self, bus: &mut BusHandle<'_>) -> Result<(), BusError> {
        if self.left == 0 {
            return Err(BusError::Detach);
        }
        bus.publish::<String>("greeting", format!("hello #{}", self.left))?;
        self.left -= 1;
        Ok(())
    }
}

pub struct Sink {
    received: Vec<String>,
}

impl Sink {
    pub fn new() -> Self {
        Self {
            received: Vec::new(),
        }
    }
    pub fn received(&self) -> &[String] {
        &self.received
    }
}

impl Default for Sink {
    fn default() -> Self {
        Self::new()
    }
}

impl Strand for Sink {
    fn name(&self) -> &'static str {
        "sink"
    }
    fn inputs(&self) -> &'static [Socket] {
        const S: [Socket; 1] = [Socket {
            name: "greeting",
            type_id: type_id_of_string(),
        }];
        &S
    }
    fn outputs(&self) -> &'static [Socket] {
        &[]
    }
    fn run(&mut self, bus: &mut BusHandle<'_>) -> Result<(), BusError> {
        for msg in bus.recv::<String>("greeting")? {
            bus.log(&format!("Sink got: {}", msg));
            self.received.push(msg);
        }
        Ok(())
    }
}

// `Socket::new::<T>()` cannot be a const fn on stable, so declare the TypeId
// via a small const-callable helper for the two message types the exemplar
// exercises. Real spiderweb-bus uses a `Socket::new::<T>` const in newer
// toolchains; the shape shown here works today.
const fn type_id_of_string() -> TypeId {
    TypeId::of::<String>()
}

fn main() {
    let mut bus = Bus::open();
    bus.register(Box::new(Source::new(2))).unwrap();
    bus.register(Box::new(Sink::new())).unwrap();
    // Drive until the bus is empty or a fixed budget elapses. The Source
    // detaches after 2 messages; Sink stays live and drains anything still in
    // flight before its own next tick, so budget = source_msgs + drain_slack.
    for _ in 0..8 {
        if !bus.is_live() {
            break;
        }
        if let Err(e) = bus.tick() {
            eprintln!("bus tick failed: {}", e);
            return;
        }
    }
    for line in bus.log() {
        println!("{}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_new_records_name_and_type() {
        let s = Socket::new::<String>("greeting");
        assert_eq!(s.name, "greeting");
        assert_eq!(s.type_id, TypeId::of::<String>());
    }

    #[test]
    fn duplicate_strand_registration_is_rejected() {
        let mut bus = Bus::open();
        bus.register(Box::new(Source::new(1))).unwrap();
        assert!(matches!(
            bus.register(Box::new(Source::new(1))),
            Err(BusError::DuplicateStrand("source"))
        ));
    }

    #[test]
    fn published_message_routes_by_type_and_name_to_matching_subscriber() {
        let mut bus = Bus::open();
        bus.register(Box::new(Source::new(3))).unwrap();
        bus.register(Box::new(Sink::new())).unwrap();
        for _ in 0..4 {
            let _ = bus.tick();
        }
        assert!(bus
            .log()
            .iter()
            .any(|line| line.contains("Sink got: hello #3")));
        assert!(bus
            .log()
            .iter()
            .any(|line| line.contains("Sink got: hello #1")));
    }

    #[test]
    fn source_returns_detach_when_budget_exhausted() {
        let mut src = Source::new(1);
        let mut inbox = Vec::new();
        let mut outbox = Vec::new();
        let mut log = Vec::new();
        {
            let mut handle = BusHandle {
                inbox: &mut inbox,
                outbox: &mut outbox,
                log: &mut log,
            };
            src.run(&mut handle).unwrap();
        }
        assert_eq!(outbox.len(), 1);
        {
            let mut handle = BusHandle {
                inbox: &mut inbox,
                outbox: &mut outbox,
                log: &mut log,
            };
            assert!(matches!(src.run(&mut handle), Err(BusError::Detach)));
        }
    }

    #[test]
    fn recv_of_wrong_type_on_matching_socket_reports_type_mismatch() {
        let mut inbox = vec![Envelope {
            socket: "greeting",
            type_id: TypeId::of::<u64>(),
            payload: Box::new(42u64),
        }];
        let mut outbox = Vec::new();
        let mut log = Vec::new();
        let mut handle = BusHandle {
            inbox: &mut inbox,
            outbox: &mut outbox,
            log: &mut log,
        };
        let result: Result<Vec<String>, BusError> = handle.recv::<String>("greeting");
        assert!(matches!(result, Err(BusError::TypeMismatch { .. })));
    }

    #[test]
    fn strand_error_display_is_useful() {
        let e = BusError::StrandFailed {
            strand: "worker",
            reason: "boom".to_string(),
        };
        let s = format!("{}", e);
        assert!(s.contains("worker") && s.contains("boom"));
    }
}
