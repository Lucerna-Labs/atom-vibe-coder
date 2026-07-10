//! Real candidate verification with immutable, independently recomputable evidence.

mod model;
mod runner;
mod store;

pub use model::{
    candidate_output, CandidateFile, CommandEvidence, RepairEvidence, VerificationAttempt,
    VerificationError, VerificationPolicy, VerificationSuccess, VerifiedCandidate,
    MAX_CANDIDATE_FILES, MAX_VERIFICATION_ATTEMPTS, VERIFICATION_SCHEMA_VERSION,
};
pub use runner::CandidateVerifier;
pub use store::verify_candidate_evidence;
