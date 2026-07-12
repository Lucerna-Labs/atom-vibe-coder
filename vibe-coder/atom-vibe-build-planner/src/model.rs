use atom_vibe_build_gates::{BlueprintRecord, LaunchEvidence, RequirementsRecord, WiringStatus};
use atom_vibe_build_protocol::{BuildArtifactRef, BuildErrorClass, BuildPlannerEvent, BuildStep};
use std::fmt;

pub const BUILD_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildRunStatus {
    Active,
    Halted,
    Complete,
}

impl BuildRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Halted => "halted",
            Self::Complete => "complete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "halted" => Some(Self::Halted),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOutput {
    pub step: BuildStep,
    pub summary: String,
    pub details: Vec<String>,
    pub artifacts: Vec<BuildArtifactRef>,
    pub recorded_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeferredDebt {
    pub step: BuildStep,
    pub reason: String,
    pub recorded_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryRecord {
    pub step: BuildStep,
    pub error_class: BuildErrorClass,
    pub attempt: u8,
    pub summary: String,
    pub artifacts: Vec<BuildArtifactRef>,
    pub decision: String,
    pub recorded_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WiringLedgerRecord {
    pub contract_id: String,
    pub status: WiringStatus,
    pub evidence: Vec<BuildArtifactRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BuildLedger {
    pub schema_version: u32,
    pub revision: u64,
    pub build_id: String,
    pub project_id: String,
    pub status: BuildRunStatus,
    pub current_step: BuildStep,
    pub autonomous_correction: bool,
    pub retry_limit: u8,
    pub requirements: Option<RequirementsRecord>,
    pub blueprint_versions: Vec<BlueprintRecord>,
    pub step_outputs: Vec<StepOutput>,
    pub crate_statuses: Vec<String>,
    pub wiring_statuses: Vec<WiringLedgerRecord>,
    pub deferred_debt: Vec<DeferredDebt>,
    pub couple_markers: Vec<String>,
    pub retry_ledger: Vec<RetryRecord>,
    pub launch_proof: Option<LaunchEvidence>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl BuildLedger {
    pub fn current_skill(&self) -> &'static str {
        self.current_step.skill_id()
    }

    pub fn retry_attempts(&self, step: BuildStep) -> usize {
        self.retry_ledger
            .iter()
            .filter(|record| record.step == step && record.decision == "retry_requested")
            .count()
    }

    pub fn projected_slice(&self) -> LedgerSlice {
        LedgerSlice {
            build_id: self.build_id.clone(),
            project_id: self.project_id.clone(),
            status: self.status,
            current_step: self.current_step,
            current_skill: self.current_skill().to_string(),
            requirements: self.requirements.clone(),
            blueprint_latest: self.blueprint_versions.last().cloned(),
            deferred_debt: self.deferred_debt.clone(),
            couple_markers: self.couple_markers.clone(),
            retry_attempts: self.retry_attempts(self.current_step),
        }
    }

    pub fn validate(&self) -> Result<(), BuildPlannerError> {
        if self.schema_version != BUILD_LEDGER_SCHEMA_VERSION || self.revision == 0 {
            return Err(BuildPlannerError::InvalidLedger(
                "schema version or revision is invalid".to_string(),
            ));
        }
        if !self.build_id.starts_with("build-")
            || self.build_id.len() > 160
            || !self
                .build_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(BuildPlannerError::InvalidLedger(
                "build id is unsafe".to_string(),
            ));
        }
        if self.project_id.trim().is_empty()
            || self.project_id.len() > 240
            || self.project_id.chars().any(|ch| ch.is_control())
        {
            return Err(BuildPlannerError::InvalidLedger(
                "project id is empty or unsafe".to_string(),
            ));
        }
        if self.retry_limit == 0 || self.retry_limit > 32 {
            return Err(BuildPlannerError::InvalidLedger(
                "retry limit must be between 1 and 32".to_string(),
            ));
        }
        if self.status == BuildRunStatus::Complete
            && (self.current_step != BuildStep::LaunchProof
                || self.launch_proof.is_none()
                || self.step_outputs.last().map(|output| output.step)
                    != Some(BuildStep::LaunchProof))
        {
            return Err(BuildPlannerError::InvalidLedger(
                "complete ledger lacks launch proof".to_string(),
            ));
        }
        if self.step_outputs.len() > BuildStep::ALL.len() {
            return Err(BuildPlannerError::InvalidLedger(
                "ledger contains too many passed step outputs".to_string(),
            ));
        }
        for (index, output) in self.step_outputs.iter().enumerate() {
            if output.step != BuildStep::ALL[index] {
                return Err(BuildPlannerError::InvalidLedger(
                    "passed step outputs are not an exact prefix".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LedgerSlice {
    pub build_id: String,
    pub project_id: String,
    pub status: BuildRunStatus,
    pub current_step: BuildStep,
    pub current_skill: String,
    pub requirements: Option<RequirementsRecord>,
    pub blueprint_latest: Option<BlueprintRecord>,
    pub deferred_debt: Vec<DeferredDebt>,
    pub couple_markers: Vec<String>,
    pub retry_attempts: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildPlannerDecision {
    Advance(BuildPlannerEvent),
    HardHalt(BuildPlannerEvent),
    Deferred(BuildPlannerEvent),
    AutonomousRetry(BuildPlannerEvent),
}

impl BuildPlannerDecision {
    pub fn event(&self) -> &BuildPlannerEvent {
        match self {
            Self::Advance(event)
            | Self::HardHalt(event)
            | Self::Deferred(event)
            | Self::AutonomousRetry(event) => event,
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Advance(_) => "advance",
            Self::HardHalt(_) => "hard_halt",
            Self::Deferred(_) => "deferred",
            Self::AutonomousRetry(_) => "autonomous_retry_requested",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildPlannerError {
    InvalidLedger(String),
    BuildNotActive,
    WrongStep {
        expected: BuildStep,
        actual: BuildStep,
    },
    DeferredOutsideCouple,
    DeferredDebtRemaining,
    CoupleMarkersRemaining,
    BuildNotFound(String),
    Store(String),
    IncompleteSpiderwebRoute,
}

impl fmt::Display for BuildPlannerError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLedger(reason) => write!(output, "invalid build ledger: {reason}"),
            Self::BuildNotActive => output.write_str("build is not active"),
            Self::WrongStep { expected, actual } => {
                write!(
                    output,
                    "gate step {actual} does not match current step {expected}"
                )
            }
            Self::DeferredOutsideCouple => {
                output.write_str("deferred outcome is only valid during crate coupling")
            }
            Self::DeferredDebtRemaining => {
                output.write_str("deferred debt must be zero before this step can advance")
            }
            Self::CoupleMarkersRemaining => {
                output.write_str("COUPLE markers must be zero before this step can advance")
            }
            Self::BuildNotFound(build_id) => write!(output, "build {build_id} was not found"),
            Self::Store(reason) => write!(output, "build ledger store failed: {reason}"),
            Self::IncompleteSpiderwebRoute => {
                output.write_str("planner event did not traverse every Spiderweb layer")
            }
        }
    }
}

impl std::error::Error for BuildPlannerError {}
