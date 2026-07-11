//! Artifact-backed build gates. Model narrative is never gate evidence.

mod evaluate;
mod model;
mod source;

pub use evaluate::evaluate_gate;
pub use model::{
    BlueprintCrate, BlueprintRecord, CommandEvidence, CrateEvidence, DependencyEdge,
    FunctionalCaseEvidence, GateInput, IndependentReview, LaunchEvidence, MessageContract,
    RequirementsRecord, RoundTripEvidence, WiringEvidence, WiringStatus,
};
