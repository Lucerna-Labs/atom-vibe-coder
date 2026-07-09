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

pub use bus::{BusLayer, BusMessageKind, Envelope, EnvelopeId, Ramp, SpiderwebBus};
pub use domain::{
    atom_by_key, atoms, gates, mission, recipes, Atom, AtomLayer, Gate, Mission, Recipe,
    RecipeStatus,
};
pub use graph::{Evidence, WikiGraph};
pub use provider::{PreparedProviderCall, ProviderConfig, ProviderError, ProviderKind};
pub use runtime::{MathAtomsRuntime, ProofRun, RuntimeState, RuntimeStatus};
