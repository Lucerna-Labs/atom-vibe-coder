//! Native Math Atoms Coder product core.
//!
//! This crate owns the non-rendering runtime: atom doctrine data, the Spiderweb Bus,
//! graph-native retrieval, provider request setup, and proof-loop state. The PMRE app
//! consumes this crate and renders the state; no browser-local state is required here.

pub mod bus;
pub mod domain;
pub mod graph;
pub mod provider;
pub mod runtime;
pub mod store;

pub use bus::{
    BackpressureSignal, BusLayer, BusMessageKind, Envelope, EnvelopeId, FabricSnapshot,
    FabricThread, Intersection, PreloadPlan, Ramp, SpiderwebBus, ThreadId, TransferPolicy,
};
pub use domain::{
    atom_by_key, atoms, gates, mission, recipes, Atom, AtomLayer, Gate, Mission, Recipe,
    RecipeStatus,
};
pub use graph::{Evidence, WikiGraph};
pub use provider::{
    provider_output_hash, PreparedProviderCall, ProviderConfig, ProviderError, ProviderKind,
};
pub use runtime::{MathAtomsRuntime, ProofRun, ProviderExecutionTask, RuntimeState, RuntimeStatus};
pub use store::{ProofRecord, ProofStore};
