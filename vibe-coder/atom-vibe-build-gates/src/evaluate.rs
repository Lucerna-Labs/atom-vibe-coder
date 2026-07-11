use crate::model::{
    BlueprintRecord, CommandEvidence, GateInput, IndependentReview, RequirementsRecord,
    WiringStatus,
};
use crate::source::{inspect_rust_sources, verify_artifacts, SourceViolation};
use atom_vibe_build_protocol::{BuildErrorClass, BuildGateEvidence, BuildStep, GateOutcome};
use math_atoms_bus::BusLayer;
use std::collections::{HashMap, HashSet};

pub fn evaluate_gate(input: &GateInput) -> GateOutcome {
    let artifacts = input.all_artifacts();
    let root = match verify_artifacts(&input.evidence_root, &artifacts) {
        Ok(root) => root,
        Err(reason) => return fail(BuildErrorClass::EvidenceInvalid, reason, Vec::new()),
    };
    let result = match input.step {
        BuildStep::Intake => evaluate_intake(input),
        BuildStep::Blueprint => evaluate_blueprint(input),
        BuildStep::CrateBuild => evaluate_crate_build(input, &root),
        BuildStep::CrateCouple => evaluate_crate_couple(input, &root),
        BuildStep::BuildTest => evaluate_build_test(input, &root),
        BuildStep::LaunchProof => evaluate_launch(input, &root),
    };
    match result {
        Ok(details) => GateOutcome::Pass {
            evidence: evidence(
                format!("{} gate passed", input.step.skill_id()),
                details,
                artifacts,
            ),
        },
        Err((class, reason)) => fail(class, reason, artifacts),
    }
}

fn evaluate_intake(input: &GateInput) -> GateCheck {
    let requirements = input.requirements.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::BoundedMechanical,
            "requirements record is missing".to_string(),
        )
    })?;
    validate_requirements(requirements)?;
    if input.blueprint.is_some() || !input.crates.is_empty() || !input.wirings.is_empty() {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "intake contains architecture or implementation work".to_string(),
        ));
    }
    Ok(vec![
        format!(
            "{} user behaviors pinned",
            requirements.user_behaviors.len()
        ),
        format!(
            "{} observable definition-of-done conditions pinned",
            requirements.definition_of_done.len()
        ),
    ])
}

fn evaluate_blueprint(input: &GateInput) -> GateCheck {
    validate_requirements(input.requirements.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::BlueprintAmendment,
            "blueprint has no complete requirements record".to_string(),
        )
    })?)?;
    let blueprint = input.blueprint.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::BlueprintAmendment,
            "blueprint record is missing".to_string(),
        )
    })?;
    validate_blueprint(blueprint)?;
    Ok(vec![
        format!("blueprint version {} frozen", blueprint.version),
        format!("{} crate responsibilities sealed", blueprint.crates.len()),
        format!(
            "{} message contracts frozen",
            blueprint.message_contracts.len()
        ),
        "independent blueprint review passed with findings resolved".to_string(),
    ])
}

fn evaluate_crate_build(input: &GateInput, root: &std::path::Path) -> GateCheck {
    let blueprint = required_blueprint(input)?;
    let expected = blueprint
        .topological_order
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let actual = input
        .crates
        .iter()
        .map(|item| item.crate_name.as_str())
        .collect::<Vec<_>>();
    if actual != expected {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "crates were not completed one at a time in frozen topological order".to_string(),
        ));
    }
    if !input.wirings.is_empty() {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "crate build contains coupling evidence before the phase boundary".to_string(),
        ));
    }
    let mut discovered_markers = Vec::new();
    for item in &input.crates {
        if item.source_artifacts.is_empty() {
            return Err((
                BuildErrorClass::UnboundedOwnership,
                format!("crate {} has no source artifacts", item.crate_name),
            ));
        }
        discovered_markers.extend(
            inspect_rust_sources(root, &item.source_artifacts, true).map_err(source_failure)?,
        );
        require_command(
            &item.cargo_check,
            &format!("{} cargo check", item.crate_name),
        )?;
        require_command(&item.unit_test, &format!("{} unit test", item.crate_name))?;
    }
    discovered_markers.sort();
    discovered_markers.dedup();
    let mut ledger_markers = input.couple_markers.clone();
    ledger_markers.sort();
    ledger_markers.dedup();
    if discovered_markers != ledger_markers {
        return Err((
            BuildErrorClass::CoupleDebt,
            "source COUPLE markers do not exactly match tracked ledger debt".to_string(),
        ));
    }
    Ok(vec![
        format!(
            "{} crates passed clean check and unit tests",
            input.crates.len()
        ),
        format!("{} scoped COUPLE markers tracked", ledger_markers.len()),
        "no crate was coupled before all crates passed".to_string(),
    ])
}

fn evaluate_crate_couple(input: &GateInput, root: &std::path::Path) -> GateCheck {
    let blueprint = required_blueprint(input)?;
    if !input.couple_markers.is_empty() {
        return Err((
            BuildErrorClass::CoupleDebt,
            "COUPLE marker list is not zero".to_string(),
        ));
    }
    let source_artifacts = input
        .crates
        .iter()
        .flat_map(|item| item.source_artifacts.clone())
        .collect::<Vec<_>>();
    inspect_rust_sources(root, &source_artifacts, false).map_err(source_failure)?;
    if input.wirings.len() != blueprint.coupling_order.len() {
        return Err((
            BuildErrorClass::CoupleDebt,
            "wiring evidence does not cover the frozen coupling order".to_string(),
        ));
    }
    let contracts = blueprint
        .message_contracts
        .iter()
        .map(|contract| (contract.id.as_str(), contract))
        .collect::<HashMap<_, _>>();
    let mut deferred = Vec::new();
    for (expected_id, wiring) in blueprint.coupling_order.iter().zip(&input.wirings) {
        if &wiring.contract_id != expected_id {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                "wirings were not verified one at a time in coupling order".to_string(),
            ));
        }
        let contract = contracts.get(expected_id.as_str()).ok_or_else(|| {
            (
                BuildErrorClass::ArchitecturalViolation,
                format!("coupling order references unknown contract {expected_id}"),
            )
        })?;
        if wiring.producer != contract.producer
            || wiring.message_type != contract.message_type
            || !contract.consumers.contains(&wiring.consumer)
        {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                format!("wiring {} violates its frozen contract", wiring.contract_id),
            ));
        }
        if wiring.direct_side_channel {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                format!("wiring {} bypasses Spiderweb Bus", wiring.contract_id),
            ));
        }
        match wiring.status {
            WiringStatus::Pass
                if wiring.message_emitted
                    && wiring.message_handled
                    && !wiring.evidence.is_empty() => {}
            WiringStatus::Pass => {
                return Err((
                    BuildErrorClass::FunctionalFailure,
                    format!(
                        "wiring {} was marked pass without observed message handling",
                        wiring.contract_id
                    ),
                ))
            }
            WiringStatus::Fail => {
                return Err((
                    BuildErrorClass::FunctionalFailure,
                    format!("wiring {} has a demonstrated failure", wiring.contract_id),
                ))
            }
            WiringStatus::Deferred if !wiring.deferred_reason.trim().is_empty() => {
                deferred.push(format!("{}:{}", wiring.contract_id, wiring.deferred_reason));
            }
            WiringStatus::Deferred => {
                return Err((
                    BuildErrorClass::CoupleDebt,
                    format!(
                        "wiring {} was deferred without a reason",
                        wiring.contract_id
                    ),
                ))
            }
        }
    }
    let mut recorded = input.deferred_debt.clone();
    recorded.sort();
    deferred.sort();
    if deferred != recorded {
        return Err((
            BuildErrorClass::CoupleDebt,
            "deferred wiring evidence does not match tracked debt".to_string(),
        ));
    }
    Ok(vec![
        format!("{} bus wirings resolved", input.wirings.len()),
        format!("{} deferred wirings tracked", deferred.len()),
        "COUPLE marker list is zero".to_string(),
    ])
}

fn evaluate_build_test(input: &GateInput, root: &std::path::Path) -> GateCheck {
    required_blueprint(input)?;
    if !input.deferred_debt.is_empty() || !input.couple_markers.is_empty() {
        return Err((
            BuildErrorClass::CoupleDebt,
            "build test cannot start with deferred debt or COUPLE markers".to_string(),
        ));
    }
    let source_artifacts = input
        .crates
        .iter()
        .flat_map(|item| item.source_artifacts.clone())
        .collect::<Vec<_>>();
    inspect_rust_sources(root, &source_artifacts, false).map_err(source_failure)?;
    for name in ["cargo-check", "cargo-test", "cargo-clippy"] {
        let command = input
            .commands
            .iter()
            .find(|command| command.name == name)
            .ok_or_else(|| {
                (
                    BuildErrorClass::CompileErrors,
                    format!("required command {name} has no captured evidence"),
                )
            })?;
        require_command(command, name)?;
    }
    let requirements = input.requirements.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::FunctionalFailure,
            "build test has no requirements record".to_string(),
        )
    })?;
    if input.functional_cases.len() < requirements.definition_of_done.len()
        || input.functional_cases.is_empty()
    {
        return Err((
            BuildErrorClass::FunctionalFailure,
            "functional evidence does not cover every definition-of-done condition".to_string(),
        ));
    }
    for case in &input.functional_cases {
        if !case.passed
            || case.smoke_only
            || !case.real_workflow
            || !case.bus_round_trip
            || case.evidence.is_empty()
        {
            return Err((
                BuildErrorClass::FunctionalFailure,
                format!(
                    "requirement {} lacks real end-to-end bus evidence",
                    case.requirement
                ),
            ));
        }
    }
    validate_review(input.implementation_review.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::AdversarialFailure,
            "implementation independent review is missing".to_string(),
        )
    })?)?;
    Ok(vec![
        "cargo check, test, and clippy passed with warnings denied".to_string(),
        format!(
            "{} real functional workflows passed over the bus",
            input.functional_cases.len()
        ),
        "independent implementation review passed with findings resolved".to_string(),
    ])
}

fn evaluate_launch(input: &GateInput, root: &std::path::Path) -> GateCheck {
    required_blueprint(input)?;
    if !input.deferred_debt.is_empty() || !input.couple_markers.is_empty() {
        return Err((
            BuildErrorClass::CoupleDebt,
            "launch proof has unresolved deferred debt or COUPLE markers".to_string(),
        ));
    }
    let source_artifacts = input
        .crates
        .iter()
        .flat_map(|item| item.source_artifacts.clone())
        .collect::<Vec<_>>();
    inspect_rust_sources(root, &source_artifacts, false).map_err(source_failure)?;
    let launch = input.launch.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::LaunchProofMissing,
            "launch evidence is missing".to_string(),
        )
    })?;
    if !launch.process_started || !launch.process_still_running || !launch.usable_screen_observed {
        return Err((
            BuildErrorClass::LaunchProofMissing,
            "app was not observed alive at a usable screen".to_string(),
        ));
    }
    if !launch.panic_free {
        return Err((
            BuildErrorClass::RuntimePanic,
            "app panicked during launch proof".to_string(),
        ));
    }
    if launch.screenshot.role != "screenshot" || launch.startup_output.role != "startup-output" {
        return Err((
            BuildErrorClass::EvidenceInvalid,
            "launch screenshot or startup output has the wrong evidence role".to_string(),
        ));
    }
    let required_layers = [
        BusLayer::L0Transport,
        BusLayer::L1Message,
        BusLayer::L2Flow,
        BusLayer::L3Orchestration,
    ];
    let round_trip = &launch.round_trip;
    if round_trip.input_event_id.trim().is_empty()
        || round_trip.runtime_result_id.trim().is_empty()
        || round_trip.evidence.is_empty()
        || required_layers
            .iter()
            .any(|layer| !round_trip.bus_layers.contains(layer))
        || round_trip.rendered_before_hash == round_trip.rendered_after_hash
        || round_trip.rendered_before_hash.trim().is_empty()
        || round_trip.rendered_after_hash.trim().is_empty()
    {
        return Err((
            BuildErrorClass::LaunchProofMissing,
            "launch proof lacks a real input, full bus route, runtime result, or rendered state change"
                .to_string(),
        ));
    }
    if !launch.definition_of_done_satisfied
        || launch.definition_of_done_evidence.is_empty()
        || !launch.completion_enforcer_clean
    {
        return Err((
            BuildErrorClass::LaunchProofMissing,
            "definition of done or completion enforcer is not proven".to_string(),
        ));
    }
    Ok(vec![
        "app remained alive at a usable screen without panic".to_string(),
        "screenshot and startup output captured".to_string(),
        "real UI input traversed L0-L3 and changed rendered state".to_string(),
        "definition of done and completion enforcer passed".to_string(),
    ])
}

type GateCheck = Result<Vec<String>, (BuildErrorClass, String)>;

fn validate_requirements(record: &RequirementsRecord) -> GateCheck {
    let scalar_fields = [
        ("purpose", record.purpose.as_str()),
        ("UI decision", record.ui_decision.as_str()),
        ("persistence decision", record.persistence_decision.as_str()),
        ("execution siting", record.execution_siting.as_str()),
    ];
    if let Some((name, _)) = scalar_fields
        .iter()
        .find(|(_, value)| value.trim().is_empty())
    {
        return Err((
            BuildErrorClass::BoundedMechanical,
            format!("requirements field {name} is blank"),
        ));
    }
    for (name, values) in [
        ("user behaviors", &record.user_behaviors),
        ("external boundaries", &record.external_boundaries),
        ("out of scope", &record.out_of_scope),
        ("definition of done", &record.definition_of_done),
    ] {
        if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
            return Err((
                BuildErrorClass::BoundedMechanical,
                format!("requirements list {name} is incomplete"),
            ));
        }
    }
    Ok(Vec::new())
}

fn validate_blueprint(blueprint: &BlueprintRecord) -> GateCheck {
    if blueprint.version == 0 || blueprint.crates.is_empty() {
        return Err((
            BuildErrorClass::BlueprintAmendment,
            "blueprint version or crate list is missing".to_string(),
        ));
    }
    let crate_names = blueprint
        .crates
        .iter()
        .map(|item| item.name.as_str())
        .collect::<HashSet<_>>();
    if crate_names.len() != blueprint.crates.len()
        || blueprint
            .crates
            .iter()
            .any(|item| item.name.trim().is_empty() || item.responsibility.trim().is_empty())
    {
        return Err((
            BuildErrorClass::UnboundedOwnership,
            "crate names are duplicate or responsibilities are blank".to_string(),
        ));
    }
    let topo = blueprint
        .topological_order
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if topo.len() != crate_names.len()
        || topo.iter().copied().collect::<HashSet<_>>() != crate_names
    {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "topological order does not contain every crate exactly once".to_string(),
        ));
    }
    let positions = topo
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, index))
        .collect::<HashMap<_, _>>();
    for edge in &blueprint.dependency_edges {
        let Some(dependency) = positions.get(edge.dependency.as_str()) else {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                format!("unknown dependency crate {}", edge.dependency),
            ));
        };
        let Some(consumer) = positions.get(edge.consumer.as_str()) else {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                format!("unknown consumer crate {}", edge.consumer),
            ));
        };
        if dependency >= consumer {
            return Err((
                BuildErrorClass::ArchitecturalViolation,
                "dependency DAG contradicts the topological order".to_string(),
            ));
        }
    }
    if blueprint.message_contracts.is_empty() {
        return Err((
            BuildErrorClass::BlueprintAmendment,
            "blueprint has no frozen message contracts".to_string(),
        ));
    }
    let contract_ids = blueprint
        .message_contracts
        .iter()
        .map(|contract| contract.id.as_str())
        .collect::<HashSet<_>>();
    if contract_ids.len() != blueprint.message_contracts.len()
        || blueprint.message_contracts.iter().any(|contract| {
            contract.id.trim().is_empty()
                || contract.message_type.trim().is_empty()
                || !crate_names.contains(contract.producer.as_str())
                || contract.consumers.is_empty()
                || contract
                    .consumers
                    .iter()
                    .any(|consumer| !crate_names.contains(consumer.as_str()))
                || contract.failure_semantics.trim().is_empty()
        })
    {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "message contracts are duplicate, incomplete, or reference unknown crates".to_string(),
        ));
    }
    if blueprint.coupling_order.len() != contract_ids.len()
        || blueprint
            .coupling_order
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>()
            != contract_ids
    {
        return Err((
            BuildErrorClass::ArchitecturalViolation,
            "coupling order does not contain every message contract exactly once".to_string(),
        ));
    }
    validate_review(&blueprint.independent_review)?;
    Ok(Vec::new())
}

fn validate_review(review: &IndependentReview) -> GateCheck {
    if review.author_identity_hash.trim().is_empty()
        || review.reviewer_identity_hash.trim().is_empty()
        || review.author_identity_hash == review.reviewer_identity_hash
        || !review.passed
        || !review.findings_resolved
        || review.evidence.is_empty()
        || review
            .findings
            .iter()
            .any(|finding| finding.trim().is_empty())
    {
        return Err((
            BuildErrorClass::AdversarialFailure,
            "independent review identity, verdict, findings, resolution, or evidence is invalid"
                .to_string(),
        ));
    }
    Ok(Vec::new())
}

fn required_blueprint(input: &GateInput) -> Result<&BlueprintRecord, (BuildErrorClass, String)> {
    let blueprint = input.blueprint.as_ref().ok_or_else(|| {
        (
            BuildErrorClass::ArchitecturalViolation,
            "frozen blueprint is missing".to_string(),
        )
    })?;
    validate_blueprint(blueprint)?;
    Ok(blueprint)
}

fn require_command(command: &CommandEvidence, label: &str) -> GateCheck {
    if !command.real_execution {
        return Err((
            BuildErrorClass::EvidenceInvalid,
            format!("{label} is narrative, not a real command execution"),
        ));
    }
    if command.timed_out || command.exit_code != 0 {
        return Err((
            BuildErrorClass::CompileErrors,
            format!("{label} failed or timed out"),
        ));
    }
    if !command.warnings_denied || command.warning_count != 0 {
        return Err((
            BuildErrorClass::CompileWarnings,
            format!("{label} did not prove a warning-free run"),
        ));
    }
    Ok(Vec::new())
}

fn source_failure(violation: SourceViolation) -> (BuildErrorClass, String) {
    match violation {
        SourceViolation::Stub { path, marker } => (
            BuildErrorClass::StubDetected,
            format!("source {path} contains forbidden stub marker {marker}"),
        ),
        SourceViolation::Allow { path, line, reason } => (
            BuildErrorClass::CompileWarnings,
            format!("warning policy failed at {path}:{line}: {reason}"),
        ),
    }
}

fn fail(
    error_class: BuildErrorClass,
    summary: String,
    artifacts: Vec<atom_vibe_build_protocol::BuildArtifactRef>,
) -> GateOutcome {
    GateOutcome::Fail {
        error_class,
        evidence: evidence(summary, Vec::new(), artifacts),
    }
}

fn evidence(
    summary: String,
    details: Vec<String>,
    artifacts: Vec<atom_vibe_build_protocol::BuildArtifactRef>,
) -> BuildGateEvidence {
    let mut evidence = BuildGateEvidence::new(summary);
    evidence.details = details;
    evidence.artifacts = artifacts;
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use atom_vibe_build_protocol::BuildArtifactRef;
    use math_atoms_hash::sha256_hex;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        root: PathBuf,
        next: usize,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "atom-vibe-gates-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root, next: 0 }
        }

        fn artifact(&mut self, role: &str, content: &str) -> BuildArtifactRef {
            self.next += 1;
            let path = format!("evidence/{:03}-{role}.txt", self.next);
            let absolute = self.root.join(&path);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(&absolute, content).unwrap();
            BuildArtifactRef::new(path, sha256_hex(content.as_bytes()), role).unwrap()
        }

        fn source(&mut self, crate_name: &str, content: &str) -> BuildArtifactRef {
            let path = format!("source/{crate_name}/src/lib.rs");
            let absolute = self.root.join(&path);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(&absolute, content).unwrap();
            BuildArtifactRef::new(path, sha256_hex(content.as_bytes()), "rust-source").unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    fn requirements(fixture: &mut Fixture) -> RequirementsRecord {
        RequirementsRecord {
            purpose: "Track inventory".to_string(),
            user_behaviors: vec!["add and remove items".to_string()],
            ui_decision: "native PMRE dashboard".to_string(),
            persistence_decision: "persist locally".to_string(),
            external_boundaries: vec!["none".to_string()],
            execution_siting: "local".to_string(),
            out_of_scope: vec!["cloud sync".to_string()],
            definition_of_done: vec!["item survives restart".to_string()],
            artifact: fixture.artifact("requirements", "complete requirements"),
        }
    }

    fn review(fixture: &mut Fixture) -> IndependentReview {
        IndependentReview {
            author_identity_hash: "sha256:author".to_string(),
            reviewer_identity_hash: "sha256:reviewer".to_string(),
            passed: true,
            findings: Vec::new(),
            findings_resolved: true,
            evidence: vec![fixture.artifact("independent-review", "review passed")],
        }
    }

    fn blueprint(fixture: &mut Fixture) -> BlueprintRecord {
        BlueprintRecord {
            version: 1,
            crates: vec![
                BlueprintCrate {
                    name: "state".to_string(),
                    responsibility: "own inventory state".to_string(),
                },
                BlueprintCrate {
                    name: "app".to_string(),
                    responsibility: "orchestrate input and rendering".to_string(),
                },
            ],
            message_contracts: vec![MessageContract {
                id: "state-to-app".to_string(),
                message_type: "InventoryChanged".to_string(),
                producer: "state".to_string(),
                consumers: vec!["app".to_string()],
                failure_semantics: "fail closed".to_string(),
            }],
            dependency_edges: vec![DependencyEdge {
                dependency: "state".to_string(),
                consumer: "app".to_string(),
            }],
            topological_order: vec!["state".to_string(), "app".to_string()],
            coupling_order: vec!["state-to-app".to_string()],
            blueprint_artifact: fixture.artifact("blueprint", "blueprint v1"),
            protocol_artifact: fixture.artifact("protocol", "InventoryChanged"),
            independent_review: review(fixture),
        }
    }

    fn command(fixture: &mut Fixture, name: &str) -> CommandEvidence {
        CommandEvidence {
            name: name.to_string(),
            exit_code: 0,
            timed_out: false,
            warnings_denied: true,
            warning_count: 0,
            real_execution: true,
            stdout: fixture.artifact("command-stdout", "ok"),
            stderr: fixture.artifact("command-stderr", ""),
        }
    }

    fn crates(fixture: &mut Fixture, source: &str) -> Vec<CrateEvidence> {
        ["state", "app"]
            .into_iter()
            .map(|name| CrateEvidence {
                crate_name: name.to_string(),
                source_artifacts: vec![fixture.source(name, source)],
                cargo_check: command(fixture, &format!("check-{name}")),
                unit_test: command(fixture, &format!("test-{name}")),
            })
            .collect()
    }

    fn base_input(fixture: &mut Fixture, step: BuildStep) -> GateInput {
        let mut input = GateInput::new(step, &fixture.root);
        input.requirements = Some(requirements(fixture));
        input.blueprint = Some(blueprint(fixture));
        input
    }

    fn failure_class(outcome: GateOutcome) -> BuildErrorClass {
        match outcome {
            GateOutcome::Fail { error_class, .. } => error_class,
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[test]
    fn intake_requires_every_record_field_and_forbids_early_blueprint() {
        let mut fixture = Fixture::new("intake");
        let mut input = GateInput::new(BuildStep::Intake, &fixture.root);
        input.requirements = Some(requirements(&mut fixture));
        assert!(evaluate_gate(&input).is_pass());
        input.requirements.as_mut().unwrap().ui_decision.clear();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::BoundedMechanical
        );
    }

    #[test]
    fn blueprint_rejects_dag_order_and_same_reviewer_identity() {
        let mut fixture = Fixture::new("blueprint");
        let mut input = base_input(&mut fixture, BuildStep::Blueprint);
        assert!(evaluate_gate(&input).is_pass());
        input
            .blueprint
            .as_mut()
            .unwrap()
            .topological_order
            .reverse();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::ArchitecturalViolation
        );
        input
            .blueprint
            .as_mut()
            .unwrap()
            .topological_order
            .reverse();
        let blueprint = input.blueprint.as_mut().unwrap();
        blueprint.independent_review.reviewer_identity_hash =
            blueprint.independent_review.author_identity_hash.clone();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::AdversarialFailure
        );
    }

    #[test]
    fn crate_build_rejects_stubs_and_unbacked_narrative_passes() {
        let mut fixture = Fixture::new("crate-build");
        let mut input = base_input(&mut fixture, BuildStep::CrateBuild);
        input.crates = crates(&mut fixture, "pub fn run() { todo!() }\n");
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::StubDetected
        );

        let mut fixture = Fixture::new("narrative");
        let mut input = base_input(&mut fixture, BuildStep::CrateBuild);
        input.crates = crates(&mut fixture, "pub fn run() {}\n");
        input.crates[0].cargo_check.real_execution = false;
        input.crates[0].cargo_check.stdout =
            fixture.artifact("command-stdout", "the model says all checks passed");
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::EvidenceInvalid
        );
    }

    #[test]
    fn crate_build_tracks_only_scoped_couple_allows() {
        let mut fixture = Fixture::new("couple-marker");
        let mut input = base_input(&mut fixture, BuildStep::CrateBuild);
        let source = "#[allow(dead_code)] // COUPLE: app\npub fn route() {}\n";
        input.crates = crates(&mut fixture, source);
        input.couple_markers = vec![
            "source/app/src/lib.rs:1:app".to_string(),
            "source/state/src/lib.rs:1:app".to_string(),
        ];
        assert!(evaluate_gate(&input).is_pass());
        input.couple_markers.pop();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::CoupleDebt
        );
    }

    #[test]
    fn coupling_requires_observed_bus_handling_and_tracks_deferred_exactly() {
        let mut fixture = Fixture::new("coupling");
        let mut input = base_input(&mut fixture, BuildStep::CrateCouple);
        input.crates = crates(&mut fixture, "pub fn run() {}\n");
        input.wirings = vec![WiringEvidence {
            contract_id: "state-to-app".to_string(),
            producer: "state".to_string(),
            consumer: "app".to_string(),
            message_type: "InventoryChanged".to_string(),
            status: WiringStatus::Pass,
            message_emitted: true,
            message_handled: true,
            direct_side_channel: false,
            deferred_reason: String::new(),
            evidence: vec![fixture.artifact("bus-flow", "handled")],
        }];
        assert!(evaluate_gate(&input).is_pass());
        input.wirings[0].message_handled = false;
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::FunctionalFailure
        );
        input.wirings[0].status = WiringStatus::Deferred;
        input.wirings[0].deferred_reason = "awaiting round trip".to_string();
        input.deferred_debt = vec!["state-to-app:awaiting round trip".to_string()];
        assert!(evaluate_gate(&input).is_pass());
    }

    #[test]
    fn build_test_rejects_smoke_checks_and_requires_independent_review() {
        let mut fixture = Fixture::new("build-test");
        let mut input = base_input(&mut fixture, BuildStep::BuildTest);
        input.crates = crates(&mut fixture, "pub fn run() {}\n");
        input.commands = ["cargo-check", "cargo-test", "cargo-clippy"]
            .into_iter()
            .map(|name| command(&mut fixture, name))
            .collect();
        input.functional_cases = vec![FunctionalCaseEvidence {
            requirement: "item survives restart".to_string(),
            passed: true,
            smoke_only: false,
            real_workflow: true,
            bus_round_trip: true,
            evidence: vec![fixture.artifact("functional-workflow", "restart passed")],
        }];
        input.implementation_review = Some(review(&mut fixture));
        assert!(evaluate_gate(&input).is_pass());
        input.functional_cases[0].smoke_only = true;
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::FunctionalFailure
        );
    }

    #[test]
    fn launch_requires_live_full_route_and_rendered_state_change() {
        let mut fixture = Fixture::new("launch");
        let mut input = base_input(&mut fixture, BuildStep::LaunchProof);
        input.crates = crates(&mut fixture, "pub fn run() {}\n");
        input.launch = Some(LaunchEvidence {
            process_started: true,
            process_still_running: true,
            panic_free: true,
            usable_screen_observed: true,
            screenshot: fixture.artifact("screenshot", "bitmap"),
            startup_output: fixture.artifact("startup-output", "ready"),
            round_trip: RoundTripEvidence {
                input_event_id: "input-1".to_string(),
                bus_layers: vec![
                    BusLayer::L0Transport,
                    BusLayer::L1Message,
                    BusLayer::L2Flow,
                    BusLayer::L3Orchestration,
                ],
                runtime_result_id: "result-1".to_string(),
                rendered_before_hash: "before".to_string(),
                rendered_after_hash: "after".to_string(),
                evidence: vec![fixture.artifact("ui-round-trip", "input to changed UI")],
            },
            definition_of_done_satisfied: true,
            definition_of_done_evidence: vec![fixture.artifact("definition-of-done", "verified")],
            completion_enforcer_clean: true,
        });
        assert!(evaluate_gate(&input).is_pass());
        input
            .launch
            .as_mut()
            .unwrap()
            .round_trip
            .rendered_after_hash = "before".to_string();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::LaunchProofMissing
        );
    }

    #[test]
    fn any_tampered_evidence_fails_before_phase_logic() {
        let mut fixture = Fixture::new("tamper");
        let input = base_input(&mut fixture, BuildStep::Blueprint);
        let path = fixture
            .root
            .join(&input.requirements.as_ref().unwrap().artifact.path);
        fs::write(path, "tampered").unwrap();
        assert_eq!(
            failure_class(evaluate_gate(&input)),
            BuildErrorClass::EvidenceInvalid
        );
    }
}
