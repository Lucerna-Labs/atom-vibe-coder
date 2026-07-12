use crate::model::{
    BuildLedger, BuildPlannerError, BuildRunStatus, DeferredDebt, RetryRecord, StepOutput,
    WiringLedgerRecord,
};
use atom_vibe_build_gates::{
    BlueprintCrate, BlueprintRecord, DependencyEdge, IndependentReview, LaunchEvidence,
    MessageContract, RequirementsRecord, RoundTripEvidence, WiringStatus,
};
use atom_vibe_build_protocol::{BuildArtifactRef, BuildErrorClass, BuildStep};
use math_atoms_bus::BusLayer;
use math_atoms_hash::sha256_tagged;
use math_atoms_json::{parse, JsonValue};
use math_atoms_lock::acquire_file_lease;
use math_atoms_secrets::redact_sensitive_text;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct BuildLedgerStore {
    root: PathBuf,
}

impl BuildLedgerStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, BuildLedgerStoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn default_store() -> Result<Self, BuildLedgerStoreError> {
        Self::open(default_ledger_root())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(&self, ledger: &BuildLedger) -> Result<PathBuf, BuildLedgerStoreError> {
        ledger
            .validate()
            .map_err(|error| BuildLedgerStoreError::Invalid(error.to_string()))?;
        let _lease = self.acquire(&ledger.build_id)?;
        let current = self.load_unlocked_optional(&ledger.build_id)?;
        if let Some((existing, path, _)) = &current {
            if existing == ledger {
                return Ok(path.clone());
            }
            if ledger.revision != existing.revision.saturating_add(1) {
                return Err(BuildLedgerStoreError::Conflict(format!(
                    "ledger revision {} does not follow persisted revision {}",
                    ledger.revision, existing.revision
                )));
            }
        } else if ledger.revision != 1 {
            return Err(BuildLedgerStoreError::Conflict(
                "first ledger snapshot must have revision 1".to_string(),
            ));
        }

        let previous_hash = current
            .as_ref()
            .map(|(_, _, bytes)| sha256_tagged(bytes))
            .unwrap_or_default();
        let ledger_json = ledger_to_json(ledger);
        let ledger_hash = sha256_tagged(ledger_json.as_bytes());
        let snapshot = format!(
            "{{\"snapshot_schema_version\":{SNAPSHOT_SCHEMA_VERSION},\"revision\":{},\"previous_snapshot_hash\":\"{}\",\"ledger_hash\":\"{}\",\"ledger\":{}}}",
            ledger.revision,
            json_escape(&previous_hash),
            json_escape(&ledger_hash),
            ledger_json
        );
        let snapshot_hash = sha256_tagged(snapshot.as_bytes());
        let path = self.snapshot_dir(&ledger.build_id).join(format!(
            "{:06}-{}.json",
            ledger.revision,
            &snapshot_hash["sha256:".len().."sha256:".len() + 12]
        ));
        write_new_atomic(&path, snapshot.as_bytes())?;
        let (readback, readback_path, _) = self
            .load_unlocked_optional(&ledger.build_id)?
            .ok_or_else(|| {
                BuildLedgerStoreError::Invalid("saved ledger disappeared".to_string())
            })?;
        if &readback != ledger || readback_path != path {
            return Err(BuildLedgerStoreError::Invalid(
                "ledger snapshot readback changed".to_string(),
            ));
        }
        Ok(path)
    }

    pub fn load(&self, build_id: &str) -> Result<BuildLedger, BuildLedgerStoreError> {
        validate_build_id(build_id)?;
        let _lease = self.acquire(build_id)?;
        self.load_unlocked_optional(build_id)?
            .map(|(ledger, _, _)| ledger)
            .ok_or_else(|| BuildLedgerStoreError::NotFound(build_id.to_string()))
    }

    pub fn list(&self) -> Result<Vec<BuildLedger>, BuildLedgerStoreError> {
        let mut ledgers = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let build_id = entry.file_name().to_string_lossy().to_string();
            if validate_build_id(&build_id).is_err() {
                continue;
            }
            ledgers.push(self.load(&build_id)?);
        }
        ledgers.sort_by_key(|ledger| std::cmp::Reverse(ledger.updated_at_unix_ms));
        Ok(ledgers)
    }

    fn load_unlocked_optional(
        &self,
        build_id: &str,
    ) -> Result<Option<(BuildLedger, PathBuf, Vec<u8>)>, BuildLedgerStoreError> {
        validate_build_id(build_id)?;
        let directory = self.snapshot_dir(build_id);
        if !directory.is_dir() {
            return Ok(None);
        }
        let mut paths = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return Ok(None);
        }
        let mut previous_hash = String::new();
        let mut latest = None;
        for (index, path) in paths.into_iter().enumerate() {
            let bytes = fs::read(&path)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                BuildLedgerStoreError::Invalid(format!("snapshot {} is not UTF-8", path.display()))
            })?;
            let root = parse(text).map_err(|error| {
                BuildLedgerStoreError::Invalid(format!(
                    "snapshot {} JSON failed: {error}",
                    path.display()
                ))
            })?;
            let schema = number_field(&root, "snapshot_schema_version")? as u32;
            let revision = number_field(&root, "revision")?;
            let recorded_previous = string_field(&root, "previous_snapshot_hash")?;
            let ledger_hash = string_field(&root, "ledger_hash")?;
            let ledger_value = root.get("ledger").ok_or_else(|| {
                BuildLedgerStoreError::Invalid("snapshot ledger missing".to_string())
            })?;
            let ledger = ledger_from_json(ledger_value)?;
            let canonical_ledger = ledger_to_json(&ledger);
            let snapshot_hash = sha256_tagged(&bytes);
            let expected_name = format!(
                "{:06}-{}.json",
                revision,
                &snapshot_hash["sha256:".len().."sha256:".len() + 12]
            );
            if schema != SNAPSHOT_SCHEMA_VERSION
                || revision != index as u64 + 1
                || ledger.revision != revision
                || ledger.build_id != build_id
                || recorded_previous != previous_hash
                || ledger_hash != sha256_tagged(canonical_ledger.as_bytes())
                || path.file_name().and_then(|value| value.to_str()) != Some(expected_name.as_str())
            {
                return Err(BuildLedgerStoreError::Invalid(format!(
                    "snapshot chain failed at {}",
                    path.display()
                )));
            }
            ledger
                .validate()
                .map_err(|error| BuildLedgerStoreError::Invalid(error.to_string()))?;
            previous_hash = snapshot_hash;
            latest = Some((ledger, path, bytes));
        }
        Ok(latest)
    }

    fn snapshot_dir(&self, build_id: &str) -> PathBuf {
        self.root.join(build_id).join("snapshots")
    }

    fn acquire(&self, build_id: &str) -> Result<math_atoms_lock::FileLease, BuildLedgerStoreError> {
        validate_build_id(build_id)?;
        acquire_file_lease(
            self.root.join(build_id).join("ledger.lock"),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(BuildLedgerStoreError::from)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildLedgerStoreError {
    NotFound(String),
    Conflict(String),
    Invalid(String),
    Io(String),
}

impl fmt::Display for BuildLedgerStoreError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(build_id) => write!(output, "build ledger {build_id} was not found"),
            Self::Conflict(reason) => write!(output, "build ledger conflict: {reason}"),
            Self::Invalid(reason) => write!(output, "build ledger evidence is invalid: {reason}"),
            Self::Io(reason) => write!(output, "build ledger I/O failed: {reason}"),
        }
    }
}

impl std::error::Error for BuildLedgerStoreError {}

impl From<std::io::Error> for BuildLedgerStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(redact_sensitive_text(&error.to_string()))
    }
}

impl From<BuildPlannerError> for BuildLedgerStoreError {
    fn from(error: BuildPlannerError) -> Self {
        Self::Invalid(error.to_string())
    }
}

pub fn default_ledger_root() -> PathBuf {
    if let Some(path) = non_empty_env("MATH_ATOMS_BUILD_LEDGER_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = non_empty_env("MATH_ATOMS_STORE_DIR") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("build-ledgers");
    }
    if let Some(path) = non_empty_env("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("build-ledgers");
    }
    std::env::temp_dir()
        .join("MathAtomsCoder")
        .join("build-ledgers")
}

fn ledger_to_json(ledger: &BuildLedger) -> String {
    format!(
        "{{\"schema_version\":{},\"revision\":{},\"build_id\":{},\"project_id\":{},\"status\":{},\"current_step\":{},\"autonomous_correction\":{},\"retry_limit\":{},\"requirements\":{},\"blueprint_versions\":{},\"step_outputs\":{},\"crate_statuses\":{},\"wiring_statuses\":{},\"deferred_debt\":{},\"couple_markers\":{},\"retry_ledger\":{},\"launch_proof\":{},\"created_at_unix_ms\":{},\"updated_at_unix_ms\":{}}}",
        ledger.schema_version,
        ledger.revision,
        q(&ledger.build_id),
        q(&ledger.project_id),
        q(ledger.status.as_str()),
        q(ledger.current_step.as_str()),
        ledger.autonomous_correction,
        ledger.retry_limit,
        ledger
            .requirements
            .as_ref()
            .map(requirements_json)
            .unwrap_or_else(|| "null".to_string()),
        array(ledger.blueprint_versions.iter().map(blueprint_json)),
        array(ledger.step_outputs.iter().map(step_output_json)),
        string_array(&ledger.crate_statuses),
        array(ledger.wiring_statuses.iter().map(wiring_json)),
        array(ledger.deferred_debt.iter().map(debt_json)),
        string_array(&ledger.couple_markers),
        array(ledger.retry_ledger.iter().map(retry_json)),
        ledger
            .launch_proof
            .as_ref()
            .map(launch_json)
            .unwrap_or_else(|| "null".to_string()),
        ledger.created_at_unix_ms,
        ledger.updated_at_unix_ms
    )
}

fn ledger_from_json(root: &JsonValue) -> Result<BuildLedger, BuildLedgerStoreError> {
    let status_text = string_field(root, "status")?;
    let step_text = string_field(root, "current_step")?;
    Ok(BuildLedger {
        schema_version: number_field(root, "schema_version")? as u32,
        revision: number_field(root, "revision")?,
        build_id: string_field(root, "build_id")?,
        project_id: string_field(root, "project_id")?,
        status: BuildRunStatus::parse(&status_text).ok_or_else(|| {
            BuildLedgerStoreError::Invalid(format!("unknown build status {status_text}"))
        })?,
        current_step: BuildStep::parse(&step_text).ok_or_else(|| {
            BuildLedgerStoreError::Invalid(format!("unknown build step {step_text}"))
        })?,
        autonomous_correction: bool_field(root, "autonomous_correction")?,
        retry_limit: number_field(root, "retry_limit")? as u8,
        requirements: optional_object(root, "requirements")?
            .map(requirements_from_json)
            .transpose()?,
        blueprint_versions: object_array(root, "blueprint_versions", blueprint_from_json)?,
        step_outputs: object_array(root, "step_outputs", step_output_from_json)?,
        crate_statuses: string_array_field(root, "crate_statuses")?,
        wiring_statuses: object_array(root, "wiring_statuses", wiring_from_json)?,
        deferred_debt: object_array(root, "deferred_debt", debt_from_json)?,
        couple_markers: string_array_field(root, "couple_markers")?,
        retry_ledger: object_array(root, "retry_ledger", retry_from_json)?,
        launch_proof: optional_object(root, "launch_proof")?
            .map(launch_from_json)
            .transpose()?,
        created_at_unix_ms: number_field(root, "created_at_unix_ms")?,
        updated_at_unix_ms: number_field(root, "updated_at_unix_ms")?,
    })
}

fn requirements_json(item: &RequirementsRecord) -> String {
    format!(
        "{{\"purpose\":{},\"user_behaviors\":{},\"ui_decision\":{},\"persistence_decision\":{},\"external_boundaries\":{},\"execution_siting\":{},\"out_of_scope\":{},\"definition_of_done\":{},\"artifact\":{}}}",
        q(&item.purpose),
        string_array(&item.user_behaviors),
        q(&item.ui_decision),
        q(&item.persistence_decision),
        string_array(&item.external_boundaries),
        q(&item.execution_siting),
        string_array(&item.out_of_scope),
        string_array(&item.definition_of_done),
        artifact_json(&item.artifact)
    )
}

fn requirements_from_json(root: &JsonValue) -> Result<RequirementsRecord, BuildLedgerStoreError> {
    Ok(RequirementsRecord {
        purpose: string_field(root, "purpose")?,
        user_behaviors: string_array_field(root, "user_behaviors")?,
        ui_decision: string_field(root, "ui_decision")?,
        persistence_decision: string_field(root, "persistence_decision")?,
        external_boundaries: string_array_field(root, "external_boundaries")?,
        execution_siting: string_field(root, "execution_siting")?,
        out_of_scope: string_array_field(root, "out_of_scope")?,
        definition_of_done: string_array_field(root, "definition_of_done")?,
        artifact: artifact_from_json(object_field(root, "artifact")?)?,
    })
}

fn blueprint_json(item: &BlueprintRecord) -> String {
    format!(
        "{{\"version\":{},\"crates\":{},\"message_contracts\":{},\"dependency_edges\":{},\"topological_order\":{},\"coupling_order\":{},\"blueprint_artifact\":{},\"protocol_artifact\":{},\"independent_review\":{}}}",
        item.version,
        array(item.crates.iter().map(|item| format!(
            "{{\"name\":{},\"responsibility\":{}}}",
            q(&item.name),
            q(&item.responsibility)
        ))),
        array(item.message_contracts.iter().map(contract_json)),
        array(item.dependency_edges.iter().map(|edge| format!(
            "{{\"dependency\":{},\"consumer\":{}}}",
            q(&edge.dependency),
            q(&edge.consumer)
        ))),
        string_array(&item.topological_order),
        string_array(&item.coupling_order),
        artifact_json(&item.blueprint_artifact),
        artifact_json(&item.protocol_artifact),
        review_json(&item.independent_review)
    )
}

fn blueprint_from_json(root: &JsonValue) -> Result<BlueprintRecord, BuildLedgerStoreError> {
    Ok(BlueprintRecord {
        version: number_field(root, "version")? as u32,
        crates: object_array(root, "crates", |value| {
            Ok(BlueprintCrate {
                name: string_field(value, "name")?,
                responsibility: string_field(value, "responsibility")?,
            })
        })?,
        message_contracts: object_array(root, "message_contracts", contract_from_json)?,
        dependency_edges: object_array(root, "dependency_edges", |value| {
            Ok(DependencyEdge {
                dependency: string_field(value, "dependency")?,
                consumer: string_field(value, "consumer")?,
            })
        })?,
        topological_order: string_array_field(root, "topological_order")?,
        coupling_order: string_array_field(root, "coupling_order")?,
        blueprint_artifact: artifact_from_json(object_field(root, "blueprint_artifact")?)?,
        protocol_artifact: artifact_from_json(object_field(root, "protocol_artifact")?)?,
        independent_review: review_from_json(object_field(root, "independent_review")?)?,
    })
}

fn contract_json(item: &MessageContract) -> String {
    format!(
        "{{\"id\":{},\"message_type\":{},\"producer\":{},\"consumers\":{},\"failure_semantics\":{}}}",
        q(&item.id),
        q(&item.message_type),
        q(&item.producer),
        string_array(&item.consumers),
        q(&item.failure_semantics)
    )
}

fn contract_from_json(root: &JsonValue) -> Result<MessageContract, BuildLedgerStoreError> {
    Ok(MessageContract {
        id: string_field(root, "id")?,
        message_type: string_field(root, "message_type")?,
        producer: string_field(root, "producer")?,
        consumers: string_array_field(root, "consumers")?,
        failure_semantics: string_field(root, "failure_semantics")?,
    })
}

fn review_json(item: &IndependentReview) -> String {
    format!(
        "{{\"author_identity_hash\":{},\"reviewer_identity_hash\":{},\"passed\":{},\"findings\":{},\"findings_resolved\":{},\"evidence\":{}}}",
        q(&item.author_identity_hash),
        q(&item.reviewer_identity_hash),
        item.passed,
        string_array(&item.findings),
        item.findings_resolved,
        array(item.evidence.iter().map(artifact_json))
    )
}

fn review_from_json(root: &JsonValue) -> Result<IndependentReview, BuildLedgerStoreError> {
    Ok(IndependentReview {
        author_identity_hash: string_field(root, "author_identity_hash")?,
        reviewer_identity_hash: string_field(root, "reviewer_identity_hash")?,
        passed: bool_field(root, "passed")?,
        findings: string_array_field(root, "findings")?,
        findings_resolved: bool_field(root, "findings_resolved")?,
        evidence: object_array(root, "evidence", artifact_from_json)?,
    })
}

fn step_output_json(item: &StepOutput) -> String {
    format!(
        "{{\"step\":{},\"summary\":{},\"details\":{},\"artifacts\":{},\"recorded_at_unix_ms\":{}}}",
        q(item.step.as_str()),
        q(&item.summary),
        string_array(&item.details),
        array(item.artifacts.iter().map(artifact_json)),
        item.recorded_at_unix_ms
    )
}

fn step_output_from_json(root: &JsonValue) -> Result<StepOutput, BuildLedgerStoreError> {
    let step = string_field(root, "step")?;
    Ok(StepOutput {
        step: BuildStep::parse(&step)
            .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("unknown step {step}")))?,
        summary: string_field(root, "summary")?,
        details: string_array_field(root, "details")?,
        artifacts: object_array(root, "artifacts", artifact_from_json)?,
        recorded_at_unix_ms: number_field(root, "recorded_at_unix_ms")?,
    })
}

fn wiring_json(item: &WiringLedgerRecord) -> String {
    format!(
        "{{\"contract_id\":{},\"status\":{},\"evidence\":{}}}",
        q(&item.contract_id),
        q(wiring_status_text(item.status)),
        array(item.evidence.iter().map(artifact_json))
    )
}

fn wiring_from_json(root: &JsonValue) -> Result<WiringLedgerRecord, BuildLedgerStoreError> {
    let status = string_field(root, "status")?;
    Ok(WiringLedgerRecord {
        contract_id: string_field(root, "contract_id")?,
        status: wiring_status_parse(&status)?,
        evidence: object_array(root, "evidence", artifact_from_json)?,
    })
}

fn debt_json(item: &DeferredDebt) -> String {
    format!(
        "{{\"step\":{},\"reason\":{},\"recorded_at_unix_ms\":{}}}",
        q(item.step.as_str()),
        q(&item.reason),
        item.recorded_at_unix_ms
    )
}

fn debt_from_json(root: &JsonValue) -> Result<DeferredDebt, BuildLedgerStoreError> {
    let step = string_field(root, "step")?;
    Ok(DeferredDebt {
        step: BuildStep::parse(&step)
            .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("unknown step {step}")))?,
        reason: string_field(root, "reason")?,
        recorded_at_unix_ms: number_field(root, "recorded_at_unix_ms")?,
    })
}

fn retry_json(item: &RetryRecord) -> String {
    format!(
        "{{\"step\":{},\"error_class\":{},\"attempt\":{},\"summary\":{},\"artifacts\":{},\"decision\":{},\"recorded_at_unix_ms\":{}}}",
        q(item.step.as_str()),
        q(item.error_class.as_str()),
        item.attempt,
        q(&item.summary),
        array(item.artifacts.iter().map(artifact_json)),
        q(&item.decision),
        item.recorded_at_unix_ms
    )
}

fn retry_from_json(root: &JsonValue) -> Result<RetryRecord, BuildLedgerStoreError> {
    let step = string_field(root, "step")?;
    let class = string_field(root, "error_class")?;
    Ok(RetryRecord {
        step: BuildStep::parse(&step)
            .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("unknown step {step}")))?,
        error_class: BuildErrorClass::parse(&class).ok_or_else(|| {
            BuildLedgerStoreError::Invalid(format!("unknown error class {class}"))
        })?,
        attempt: number_field(root, "attempt")? as u8,
        summary: string_field(root, "summary")?,
        artifacts: object_array(root, "artifacts", artifact_from_json)?,
        decision: string_field(root, "decision")?,
        recorded_at_unix_ms: number_field(root, "recorded_at_unix_ms")?,
    })
}

fn launch_json(item: &LaunchEvidence) -> String {
    let round_trip = &item.round_trip;
    format!(
        "{{\"process_started\":{},\"process_still_running\":{},\"panic_free\":{},\"usable_screen_observed\":{},\"screenshot\":{},\"startup_output\":{},\"round_trip\":{{\"input_event_id\":{},\"bus_layers\":{},\"runtime_result_id\":{},\"rendered_before_hash\":{},\"rendered_after_hash\":{},\"evidence\":{}}},\"definition_of_done_satisfied\":{},\"definition_of_done_evidence\":{},\"completion_enforcer_clean\":{}}}",
        item.process_started,
        item.process_still_running,
        item.panic_free,
        item.usable_screen_observed,
        artifact_json(&item.screenshot),
        artifact_json(&item.startup_output),
        q(&round_trip.input_event_id),
        array(round_trip.bus_layers.iter().map(|layer| q(bus_layer_text(*layer)))),
        q(&round_trip.runtime_result_id),
        q(&round_trip.rendered_before_hash),
        q(&round_trip.rendered_after_hash),
        array(round_trip.evidence.iter().map(artifact_json)),
        item.definition_of_done_satisfied,
        array(item.definition_of_done_evidence.iter().map(artifact_json)),
        item.completion_enforcer_clean
    )
}

fn launch_from_json(root: &JsonValue) -> Result<LaunchEvidence, BuildLedgerStoreError> {
    let round_trip = object_field(root, "round_trip")?;
    let layers = string_array_field(round_trip, "bus_layers")?
        .iter()
        .map(|value| bus_layer_parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LaunchEvidence {
        process_started: bool_field(root, "process_started")?,
        process_still_running: bool_field(root, "process_still_running")?,
        panic_free: bool_field(root, "panic_free")?,
        usable_screen_observed: bool_field(root, "usable_screen_observed")?,
        screenshot: artifact_from_json(object_field(root, "screenshot")?)?,
        startup_output: artifact_from_json(object_field(root, "startup_output")?)?,
        round_trip: RoundTripEvidence {
            input_event_id: string_field(round_trip, "input_event_id")?,
            bus_layers: layers,
            runtime_result_id: string_field(round_trip, "runtime_result_id")?,
            rendered_before_hash: string_field(round_trip, "rendered_before_hash")?,
            rendered_after_hash: string_field(round_trip, "rendered_after_hash")?,
            evidence: object_array(round_trip, "evidence", artifact_from_json)?,
        },
        definition_of_done_satisfied: bool_field(root, "definition_of_done_satisfied")?,
        definition_of_done_evidence: object_array(
            root,
            "definition_of_done_evidence",
            artifact_from_json,
        )?,
        completion_enforcer_clean: bool_field(root, "completion_enforcer_clean")?,
    })
}

fn artifact_json(item: &BuildArtifactRef) -> String {
    format!(
        "{{\"path\":{},\"sha256_hex\":{},\"role\":{}}}",
        q(&item.path),
        q(&item.sha256_hex),
        q(&item.role)
    )
}

fn artifact_from_json(root: &JsonValue) -> Result<BuildArtifactRef, BuildLedgerStoreError> {
    BuildArtifactRef::new(
        string_field(root, "path")?,
        string_field(root, "sha256_hex")?,
        string_field(root, "role")?,
    )
    .map_err(|error| BuildLedgerStoreError::Invalid(error.to_string()))
}

fn wiring_status_text(status: WiringStatus) -> &'static str {
    match status {
        WiringStatus::Pass => "pass",
        WiringStatus::Fail => "fail",
        WiringStatus::Deferred => "deferred",
    }
}

fn wiring_status_parse(value: &str) -> Result<WiringStatus, BuildLedgerStoreError> {
    match value {
        "pass" => Ok(WiringStatus::Pass),
        "fail" => Ok(WiringStatus::Fail),
        "deferred" => Ok(WiringStatus::Deferred),
        _ => Err(BuildLedgerStoreError::Invalid(format!(
            "unknown wiring status {value}"
        ))),
    }
}

fn bus_layer_text(layer: BusLayer) -> &'static str {
    match layer {
        BusLayer::L0Transport => "l0_transport",
        BusLayer::L1Message => "l1_message",
        BusLayer::L2Flow => "l2_flow",
        BusLayer::L3Orchestration => "l3_orchestration",
    }
}

fn bus_layer_parse(value: &str) -> Result<BusLayer, BuildLedgerStoreError> {
    match value {
        "l0_transport" => Ok(BusLayer::L0Transport),
        "l1_message" => Ok(BusLayer::L1Message),
        "l2_flow" => Ok(BusLayer::L2Flow),
        "l3_orchestration" => Ok(BusLayer::L3Orchestration),
        _ => Err(BuildLedgerStoreError::Invalid(format!(
            "unknown bus layer {value}"
        ))),
    }
}

fn q(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn array(values: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", values.into_iter().collect::<Vec<_>>().join(","))
}

fn string_array(values: &[String]) -> String {
    array(values.iter().map(|value| q(value)))
}

fn string_field(root: &JsonValue, key: &str) -> Result<String, BuildLedgerStoreError> {
    root.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("missing string field {key}")))
}

fn number_field(root: &JsonValue, key: &str) -> Result<u64, BuildLedgerStoreError> {
    root.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("missing number field {key}")))
}

fn bool_field(root: &JsonValue, key: &str) -> Result<bool, BuildLedgerStoreError> {
    match root.get(key) {
        Some(JsonValue::Bool(value)) => Ok(*value),
        _ => Err(BuildLedgerStoreError::Invalid(format!(
            "missing boolean field {key}"
        ))),
    }
}

fn object_field<'a>(
    root: &'a JsonValue,
    key: &str,
) -> Result<&'a JsonValue, BuildLedgerStoreError> {
    let value = root
        .get(key)
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("missing object field {key}")))?;
    value
        .as_object()
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("field {key} is not an object")))?;
    Ok(value)
}

fn optional_object<'a>(
    root: &'a JsonValue,
    key: &str,
) -> Result<Option<&'a JsonValue>, BuildLedgerStoreError> {
    match root.get(key) {
        Some(JsonValue::Null) => Ok(None),
        Some(value) if value.as_object().is_some() => Ok(Some(value)),
        _ => Err(BuildLedgerStoreError::Invalid(format!(
            "field {key} is not an object or null"
        ))),
    }
}

fn string_array_field(root: &JsonValue, key: &str) -> Result<Vec<String>, BuildLedgerStoreError> {
    root.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("field {key} is not an array")))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                BuildLedgerStoreError::Invalid(format!("field {key} contains a non-string"))
            })
        })
        .collect()
}

fn object_array<T>(
    root: &JsonValue,
    key: &str,
    parser: impl Fn(&JsonValue) -> Result<T, BuildLedgerStoreError>,
) -> Result<Vec<T>, BuildLedgerStoreError> {
    root.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| BuildLedgerStoreError::Invalid(format!("field {key} is not an array")))?
        .iter()
        .map(|value| {
            value.as_object().ok_or_else(|| {
                BuildLedgerStoreError::Invalid(format!("field {key} contains a non-object"))
            })?;
            parser(value)
        })
        .collect()
}

fn validate_build_id(value: &str) -> Result<(), BuildLedgerStoreError> {
    if !value.starts_with("build-")
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(BuildLedgerStoreError::Invalid(format!(
            "unsafe build id {value}"
        )));
    }
    Ok(())
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> Result<(), BuildLedgerStoreError> {
    if path.exists() {
        return if fs::read(path)? == bytes {
            Ok(())
        } else {
            Err(BuildLedgerStoreError::Conflict(format!(
                "immutable snapshot conflict at {}",
                path.display()
            )))
        };
    }
    let parent = path
        .parent()
        .ok_or_else(|| BuildLedgerStoreError::Io("snapshot path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".ledger-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(temp);
        return Err(error.into());
    }
    if fs::read(path)? != bytes {
        return Err(BuildLedgerStoreError::Invalid(
            "snapshot readback failed".to_string(),
        ));
    }
    Ok(())
}

fn json_escape(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push(' '),
            ch => output.push(ch),
        }
    }
    output
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildPlanner;

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-ledger-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn append_only_snapshots_round_trip_and_detect_tampering() {
        let root = root("roundtrip");
        let store = BuildLedgerStore::open(&root).unwrap();
        let planner = BuildPlanner::new("inventory", true, 6).unwrap();
        let build_id = planner.ledger().build_id.clone();
        let path = store.save(planner.ledger()).unwrap();
        assert_eq!(store.load(&build_id).unwrap(), *planner.ledger());
        assert_eq!(store.list().unwrap().len(), 1);

        let changed = fs::read_to_string(&path)
            .unwrap()
            .replace("inventory", "tampered");
        fs::write(path, changed).unwrap();
        assert!(matches!(
            store.load(&build_id),
            Err(BuildLedgerStoreError::Invalid(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn revisions_must_be_exactly_sequential() {
        let root = root("revision");
        let store = BuildLedgerStore::open(&root).unwrap();
        let planner = BuildPlanner::new("inventory", true, 6).unwrap();
        store.save(planner.ledger()).unwrap();
        let mut skipped = planner.ledger().clone();
        skipped.revision = 3;
        assert!(matches!(
            store.save(&skipped),
            Err(BuildLedgerStoreError::Conflict(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hash_tag_contract_is_used_for_snapshot_chains() {
        assert!(math_atoms_hash::valid_sha256_tag(&sha256_tagged(
            b"snapshot"
        )));
    }
}
