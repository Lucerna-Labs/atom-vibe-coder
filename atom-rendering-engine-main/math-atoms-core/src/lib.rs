//! Native Atom Vibe Coder product core.
//!
//! This crate owns the non-rendering runtime: atom doctrine data, the Spiderweb Bus,
//! graph-native retrieval, provider request setup, and proof-loop state. The PMRE app
//! consumes this crate and renders the state; no browser-local state is required here.

pub mod domain;
pub mod provider;
mod provider_verification;
pub mod runtime;

pub use domain::{
    atom_by_key, atoms, gates, mission, recipes, Atom, AtomLayer, Gate, Mission, Recipe,
    RecipeStatus,
};
pub use math_atoms_bus::{
    BackpressureSignal, BusLayer, BusMessageKind, Envelope, EnvelopeId, FabricSnapshot,
    FabricThread, Intersection, PreloadPlan, Ramp, SpiderwebBus, ThreadId, TransferPolicy,
};
pub use math_atoms_graph::{Evidence, WikiGraph};
pub use math_atoms_learning::{
    artifact_hash, effective_records, rank_records, LearningHit, LearningOutcome, LearningRecord,
    LearningRecordInput, LearningStore, LearningSummary, DEFAULT_GRAPH_MEMORY_LIMIT,
};
pub use math_atoms_proof::{ProofRecord, ProofStore};
pub use math_atoms_verification::{CandidateFile, CandidateVerifier, VerificationPolicy};
pub use math_atoms_work::{
    verify_work_plan_evidence, PacketContract, WorkFile, WorkPlan, WorkPlanStore, WorkStage,
};
pub use provider::{
    default_provider_output_dir, persist_provider_output, provider_output_hash,
    CandidateVerificationReport, PersistedProviderOutput, PreparedProviderCall, ProviderConfig,
    ProviderConfigInput, ProviderError, ProviderExecutionOutput, ProviderKind,
    ProviderThinkingLevel, ProviderWireFormat,
};
pub use runtime::{MathAtomsRuntime, ProofRun, ProviderExecutionTask, RuntimeState, RuntimeStatus};
