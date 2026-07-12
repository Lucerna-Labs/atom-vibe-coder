//! Durable composition root for Atom Vibe Coder.
//!
//! The renderer remains a consumed component. This crate owns session identity,
//! model-turn orchestration, graph-plus-scratchpad context, provider evidence,
//! gate submission, and restart-safe runtime records.

mod contracts;
mod model;
mod runtime;
mod session;
mod turns;

pub use contracts::step_output_contract;
pub use model::{
    ExecutedTurn, PreparedTurn, ProviderResultRoute, RuntimeError, RuntimePaths, SessionManifest,
    TurnRecord, TURN_RECORD_SCHEMA_VERSION,
};
pub use runtime::AtomVibeRuntime;
