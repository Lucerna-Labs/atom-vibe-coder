//! Stable contracts for Atom Vibe Coder's six-stage build spine.
//!
//! The model may propose artifacts and corrections. Only independently produced
//! [`GateOutcome`] evidence can advance a build from one [`BuildStep`] to the next.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod build_topics {
    pub const STEP_COMPLETED: &str = "atom.build.step.completed";
    pub const GATE_RESULT: &str = "atom.build.gate.result";
    pub const PLANNER_EVENT: &str = "atom.build.planner.event";
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BuildStep {
    Intake,
    Blueprint,
    CrateBuild,
    CrateCouple,
    BuildTest,
    LaunchProof,
}

impl BuildStep {
    pub const ALL: [Self; 6] = [
        Self::Intake,
        Self::Blueprint,
        Self::CrateBuild,
        Self::CrateCouple,
        Self::BuildTest,
        Self::LaunchProof,
    ];

    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Intake => 1,
            Self::Blueprint => 2,
            Self::CrateBuild => 3,
            Self::CrateCouple => 4,
            Self::BuildTest => 5,
            Self::LaunchProof => 6,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Intake => "intake",
            Self::Blueprint => "blueprint",
            Self::CrateBuild => "crate_build",
            Self::CrateCouple => "crate_couple",
            Self::BuildTest => "build_test",
            Self::LaunchProof => "launch_proof",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Intake => "Intake",
            Self::Blueprint => "Blueprint",
            Self::CrateBuild => "Crate Build",
            Self::CrateCouple => "Crate Couple",
            Self::BuildTest => "Build Test",
            Self::LaunchProof => "Launch Proof",
        }
    }

    pub const fn skill_id(self) -> &'static str {
        match self {
            Self::Intake => "atom-build-intake",
            Self::Blueprint => "atom-build-blueprint",
            Self::CrateBuild => "atom-crate-build",
            Self::CrateCouple => "atom-crate-couple",
            Self::BuildTest => "atom-build-test",
            Self::LaunchProof => "atom-launch-proof",
        }
    }

    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Intake => Some(Self::Blueprint),
            Self::Blueprint => Some(Self::CrateBuild),
            Self::CrateBuild => Some(Self::CrateCouple),
            Self::CrateCouple => Some(Self::BuildTest),
            Self::BuildTest => Some(Self::LaunchProof),
            Self::LaunchProof => None,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.as_str() == value)
    }
}

impl fmt::Display for BuildStep {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildArtifactRef {
    pub path: String,
    pub sha256_hex: String,
    pub role: String,
}

impl BuildArtifactRef {
    pub fn new(
        path: impl Into<String>,
        sha256_hex: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<Self, ProtocolError> {
        let artifact = Self {
            path: path.into(),
            sha256_hex: sha256_hex.into(),
            role: role.into(),
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !is_safe_evidence_path(&self.path) {
            return Err(ProtocolError::InvalidArtifactPath(self.path.clone()));
        }
        if self.sha256_hex.len() != 64
            || !self.sha256_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ProtocolError::InvalidArtifactHash(self.path.clone()));
        }
        if self.role.trim().is_empty() || self.role.len() > 96 {
            return Err(ProtocolError::InvalidArtifactRole(self.path.clone()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildStepCompletedSignal {
    pub build_id: String,
    pub project_id: String,
    pub step: BuildStep,
    pub summary: String,
    pub artifacts: Vec<BuildArtifactRef>,
    pub output: String,
    pub completed_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildGateEvidence {
    pub summary: String,
    pub details: Vec<String>,
    pub artifacts: Vec<BuildArtifactRef>,
    pub checked_at_unix_ms: u64,
}

impl BuildGateEvidence {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            details: Vec::new(),
            artifacts: Vec::new(),
            checked_at_unix_ms: unix_time_ms(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.details.push(detail.into());
        self
    }

    pub fn with_artifact(mut self, artifact: BuildArtifactRef) -> Self {
        self.artifacts.push(artifact);
        self
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.summary.trim().is_empty() || self.summary.len() > 4 * 1024 {
            return Err(ProtocolError::InvalidEvidence(
                "gate evidence requires a bounded summary".to_string(),
            ));
        }
        if self.details.len() > 256
            || self
                .details
                .iter()
                .any(|detail| detail.trim().is_empty() || detail.len() > 16 * 1024)
        {
            return Err(ProtocolError::InvalidEvidence(
                "gate evidence details are empty or exceed bounds".to_string(),
            ));
        }
        if self.artifacts.len() > 256 {
            return Err(ProtocolError::InvalidEvidence(
                "gate evidence contains too many artifacts".to_string(),
            ));
        }
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuildErrorClass {
    BoundedMechanical,
    BlueprintAmendment,
    CompileErrors,
    CompileWarnings,
    ArchitecturalViolation,
    StubDetected,
    CoupleDebt,
    FunctionalFailure,
    AdversarialFailure,
    LaunchProofMissing,
    RuntimePanic,
    UnboundedOwnership,
    RetryExhausted,
    EvidenceInvalid,
    Unknown,
}

impl BuildErrorClass {
    pub const ALL: [Self; 15] = [
        Self::BoundedMechanical,
        Self::BlueprintAmendment,
        Self::CompileErrors,
        Self::CompileWarnings,
        Self::ArchitecturalViolation,
        Self::StubDetected,
        Self::CoupleDebt,
        Self::FunctionalFailure,
        Self::AdversarialFailure,
        Self::LaunchProofMissing,
        Self::RuntimePanic,
        Self::UnboundedOwnership,
        Self::RetryExhausted,
        Self::EvidenceInvalid,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedMechanical => "bounded_mechanical",
            Self::BlueprintAmendment => "blueprint_amendment",
            Self::CompileErrors => "compile_errors",
            Self::CompileWarnings => "compile_warnings",
            Self::ArchitecturalViolation => "architectural_violation",
            Self::StubDetected => "stub_detected",
            Self::CoupleDebt => "couple_debt",
            Self::FunctionalFailure => "functional_failure",
            Self::AdversarialFailure => "adversarial_failure",
            Self::LaunchProofMissing => "launch_proof_missing",
            Self::RuntimePanic => "runtime_panic",
            Self::UnboundedOwnership => "unbounded_ownership",
            Self::RetryExhausted => "retry_exhausted",
            Self::EvidenceInvalid => "evidence_invalid",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.as_str() == value)
    }

    pub const fn is_bounded_candidate(self) -> bool {
        matches!(
            self,
            Self::BoundedMechanical
                | Self::BlueprintAmendment
                | Self::CompileErrors
                | Self::CompileWarnings
                | Self::StubDetected
                | Self::CoupleDebt
                | Self::FunctionalFailure
                | Self::AdversarialFailure
                | Self::LaunchProofMissing
        )
    }
}

impl fmt::Display for BuildErrorClass {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateOutcome {
    Pass {
        evidence: BuildGateEvidence,
    },
    Fail {
        error_class: BuildErrorClass,
        evidence: BuildGateEvidence,
    },
    Deferred {
        reason: String,
        evidence: BuildGateEvidence,
    },
}

impl GateOutcome {
    pub const fn is_pass(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }

    pub const fn error_class(&self) -> Option<BuildErrorClass> {
        match self {
            Self::Fail { error_class, .. } => Some(*error_class),
            Self::Pass { .. } | Self::Deferred { .. } => None,
        }
    }

    pub const fn evidence(&self) -> &BuildGateEvidence {
        match self {
            Self::Pass { evidence }
            | Self::Fail { evidence, .. }
            | Self::Deferred { evidence, .. } => evidence,
        }
    }

    pub fn validate(&self, step: BuildStep) -> Result<(), ProtocolError> {
        self.evidence().validate()?;
        if let Self::Deferred { reason, .. } = self {
            if step != BuildStep::CrateCouple {
                return Err(ProtocolError::DeferredOutsideCouple);
            }
            if reason.trim().is_empty() || reason.len() > 4 * 1024 {
                return Err(ProtocolError::InvalidEvidence(
                    "deferred outcome requires a bounded reason".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildGateResult {
    pub build_id: String,
    pub project_id: String,
    pub step: BuildStep,
    pub outcome: GateOutcome,
}

impl BuildGateResult {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_identifier("build_id", &self.build_id, 160)?;
        validate_identifier("project_id", &self.project_id, 240)?;
        self.outcome.validate(self.step)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildPlannerEvent {
    BuildStarted {
        build_id: String,
        project_id: String,
        released_skill: String,
    },
    StepAdvanced {
        build_id: String,
        project_id: String,
        completed_step: BuildStep,
        next_step: Option<BuildStep>,
        released_skill: Option<String>,
    },
    DeferredDebtRecorded {
        build_id: String,
        project_id: String,
        step: BuildStep,
        reason: String,
    },
    HardHalted {
        build_id: String,
        project_id: String,
        step: BuildStep,
        error_class: BuildErrorClass,
        summary: String,
    },
    AutonomousRetryRequested {
        build_id: String,
        project_id: String,
        step: BuildStep,
        error_class: BuildErrorClass,
        attempt: u8,
        attempt_limit: u8,
        summary: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    InvalidIdentifier { field: &'static str, value: String },
    InvalidArtifactPath(String),
    InvalidArtifactHash(String),
    InvalidArtifactRole(String),
    InvalidEvidence(String),
    DeferredOutsideCouple,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier { field, value } => {
                write!(output, "invalid {field}: {value}")
            }
            Self::InvalidArtifactPath(path) => write!(output, "unsafe artifact path: {path}"),
            Self::InvalidArtifactHash(path) => {
                write!(output, "artifact hash is not SHA-256 hex: {path}")
            }
            Self::InvalidArtifactRole(path) => write!(output, "invalid artifact role: {path}"),
            Self::InvalidEvidence(reason) => write!(output, "invalid gate evidence: {reason}"),
            Self::DeferredOutsideCouple => {
                output.write_str("deferred gate outcome is only valid for crate coupling")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

pub fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn validate_identifier(
    field: &'static str,
    value: &str,
    limit: usize,
) -> Result<(), ProtocolError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > limit
        || value.chars().any(|ch| ch.is_control())
    {
        return Err(ProtocolError::InvalidIdentifier {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn is_safe_evidence_path(value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    !normalized.is_empty()
        && normalized.len() <= 1024
        && !normalized.starts_with('/')
        && !normalized.contains(':')
        && normalized
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !normalized.chars().any(|ch| ch.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash() -> String {
        "a".repeat(64)
    }

    #[test]
    fn fixed_steps_release_exactly_one_next_skill() {
        for (index, step) in BuildStep::ALL.into_iter().enumerate() {
            assert_eq!(step.ordinal() as usize, index + 1);
            assert_eq!(BuildStep::parse(step.as_str()), Some(step));
            assert!(!step.skill_id().is_empty());
            let expected = BuildStep::ALL.get(index + 1).copied();
            assert_eq!(step.next(), expected);
        }
    }

    #[test]
    fn deferred_outcome_is_only_legal_during_coupling() {
        let deferred = GateOutcome::Deferred {
            reason: "consumer crate is scheduled later".to_string(),
            evidence: BuildGateEvidence::new("coupling debt recorded"),
        };
        assert!(deferred.validate(BuildStep::CrateCouple).is_ok());
        assert_eq!(
            deferred.validate(BuildStep::BuildTest),
            Err(ProtocolError::DeferredOutsideCouple)
        );
    }

    #[test]
    fn artifacts_require_safe_paths_and_real_sha256_shape() {
        assert!(BuildArtifactRef::new("logs/check.txt", hash(), "cargo-check").is_ok());
        assert!(BuildArtifactRef::new("../secret", hash(), "bad").is_err());
        assert!(BuildArtifactRef::new("logs/check.txt", "abc", "bad").is_err());
    }

    #[test]
    fn bounded_retry_classes_are_explicit() {
        assert!(BuildErrorClass::CompileErrors.is_bounded_candidate());
        assert!(BuildErrorClass::LaunchProofMissing.is_bounded_candidate());
        assert!(!BuildErrorClass::ArchitecturalViolation.is_bounded_candidate());
        assert!(!BuildErrorClass::UnboundedOwnership.is_bounded_candidate());
    }

    #[test]
    fn result_validation_rejects_blank_identity() {
        let result = BuildGateResult {
            build_id: String::new(),
            project_id: "demo".to_string(),
            step: BuildStep::Intake,
            outcome: GateOutcome::Pass {
                evidence: BuildGateEvidence::new("intake passed"),
            },
        };
        assert!(matches!(
            result.validate(),
            Err(ProtocolError::InvalidIdentifier {
                field: "build_id",
                ..
            })
        ));
    }
}
