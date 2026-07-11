use atom_vibe_build_protocol::{BuildArtifactRef, BuildStep};
use math_atoms_bus::BusLayer;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementsRecord {
    pub purpose: String,
    pub user_behaviors: Vec<String>,
    pub ui_decision: String,
    pub persistence_decision: String,
    pub external_boundaries: Vec<String>,
    pub execution_siting: String,
    pub out_of_scope: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub artifact: BuildArtifactRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlueprintCrate {
    pub name: String,
    pub responsibility: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageContract {
    pub id: String,
    pub message_type: String,
    pub producer: String,
    pub consumers: Vec<String>,
    pub failure_semantics: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyEdge {
    pub dependency: String,
    pub consumer: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndependentReview {
    pub author_identity_hash: String,
    pub reviewer_identity_hash: String,
    pub passed: bool,
    pub findings: Vec<String>,
    pub findings_resolved: bool,
    pub evidence: Vec<BuildArtifactRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlueprintRecord {
    pub version: u32,
    pub crates: Vec<BlueprintCrate>,
    pub message_contracts: Vec<MessageContract>,
    pub dependency_edges: Vec<DependencyEdge>,
    pub topological_order: Vec<String>,
    pub coupling_order: Vec<String>,
    pub blueprint_artifact: BuildArtifactRef,
    pub protocol_artifact: BuildArtifactRef,
    pub independent_review: IndependentReview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandEvidence {
    pub name: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub warnings_denied: bool,
    pub warning_count: usize,
    pub real_execution: bool,
    pub stdout: BuildArtifactRef,
    pub stderr: BuildArtifactRef,
}

impl CommandEvidence {
    pub fn passed(&self) -> bool {
        self.real_execution
            && !self.timed_out
            && self.exit_code == 0
            && self.warnings_denied
            && self.warning_count == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrateEvidence {
    pub crate_name: String,
    pub source_artifacts: Vec<BuildArtifactRef>,
    pub cargo_check: CommandEvidence,
    pub unit_test: CommandEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WiringStatus {
    Pass,
    Fail,
    Deferred,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WiringEvidence {
    pub contract_id: String,
    pub producer: String,
    pub consumer: String,
    pub message_type: String,
    pub status: WiringStatus,
    pub message_emitted: bool,
    pub message_handled: bool,
    pub direct_side_channel: bool,
    pub deferred_reason: String,
    pub evidence: Vec<BuildArtifactRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionalCaseEvidence {
    pub requirement: String,
    pub passed: bool,
    pub smoke_only: bool,
    pub real_workflow: bool,
    pub bus_round_trip: bool,
    pub evidence: Vec<BuildArtifactRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundTripEvidence {
    pub input_event_id: String,
    pub bus_layers: Vec<BusLayer>,
    pub runtime_result_id: String,
    pub rendered_before_hash: String,
    pub rendered_after_hash: String,
    pub evidence: Vec<BuildArtifactRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchEvidence {
    pub process_started: bool,
    pub process_still_running: bool,
    pub panic_free: bool,
    pub usable_screen_observed: bool,
    pub screenshot: BuildArtifactRef,
    pub startup_output: BuildArtifactRef,
    pub round_trip: RoundTripEvidence,
    pub definition_of_done_satisfied: bool,
    pub definition_of_done_evidence: Vec<BuildArtifactRef>,
    pub completion_enforcer_clean: bool,
}

#[derive(Clone, Debug)]
pub struct GateInput {
    pub step: BuildStep,
    pub evidence_root: PathBuf,
    pub requirements: Option<RequirementsRecord>,
    pub blueprint: Option<BlueprintRecord>,
    pub crates: Vec<CrateEvidence>,
    pub wirings: Vec<WiringEvidence>,
    pub commands: Vec<CommandEvidence>,
    pub functional_cases: Vec<FunctionalCaseEvidence>,
    pub implementation_review: Option<IndependentReview>,
    pub couple_markers: Vec<String>,
    pub deferred_debt: Vec<String>,
    pub launch: Option<LaunchEvidence>,
}

impl GateInput {
    pub fn new(step: BuildStep, evidence_root: impl Into<PathBuf>) -> Self {
        Self {
            step,
            evidence_root: evidence_root.into(),
            requirements: None,
            blueprint: None,
            crates: Vec::new(),
            wirings: Vec::new(),
            commands: Vec::new(),
            functional_cases: Vec::new(),
            implementation_review: None,
            couple_markers: Vec::new(),
            deferred_debt: Vec::new(),
            launch: None,
        }
    }

    pub(crate) fn all_artifacts(&self) -> Vec<BuildArtifactRef> {
        let mut artifacts = Vec::new();
        if let Some(requirements) = &self.requirements {
            artifacts.push(requirements.artifact.clone());
        }
        if let Some(blueprint) = &self.blueprint {
            artifacts.push(blueprint.blueprint_artifact.clone());
            artifacts.push(blueprint.protocol_artifact.clone());
            artifacts.extend(blueprint.independent_review.evidence.clone());
        }
        for item in &self.crates {
            artifacts.extend(item.source_artifacts.clone());
            push_command_artifacts(&mut artifacts, &item.cargo_check);
            push_command_artifacts(&mut artifacts, &item.unit_test);
        }
        for wiring in &self.wirings {
            artifacts.extend(wiring.evidence.clone());
        }
        for command in &self.commands {
            push_command_artifacts(&mut artifacts, command);
        }
        for case in &self.functional_cases {
            artifacts.extend(case.evidence.clone());
        }
        if let Some(review) = &self.implementation_review {
            artifacts.extend(review.evidence.clone());
        }
        if let Some(launch) = &self.launch {
            artifacts.push(launch.screenshot.clone());
            artifacts.push(launch.startup_output.clone());
            artifacts.extend(launch.round_trip.evidence.clone());
            artifacts.extend(launch.definition_of_done_evidence.clone());
        }
        artifacts.sort_by(|left, right| {
            (&left.path, &left.sha256_hex, &left.role).cmp(&(
                &right.path,
                &right.sha256_hex,
                &right.role,
            ))
        });
        artifacts.dedup();
        artifacts
    }
}

fn push_command_artifacts(artifacts: &mut Vec<BuildArtifactRef>, command: &CommandEvidence) {
    artifacts.push(command.stdout.clone());
    artifacts.push(command.stderr.clone());
}
