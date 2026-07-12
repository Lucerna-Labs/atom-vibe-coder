use atom_vibe_build_gates::{
    BlueprintCrate, BlueprintRecord, CommandEvidence, CrateEvidence, DependencyEdge,
    FunctionalCaseEvidence, GateInput, IndependentReview, LaunchEvidence, MessageContract,
    RequirementsRecord, RoundTripEvidence, WiringEvidence, WiringStatus,
};
use atom_vibe_build_planner::{BuildCoordinator, BuildPlannerDecision, BuildRunStatus};
use atom_vibe_build_protocol::{BuildArtifactRef, BuildStep};
use math_atoms_bus::BusLayer;
use math_atoms_hash::sha256_hex;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    counter: usize,
    requirements: RequirementsRecord,
    blueprint: BlueprintRecord,
    crates: Vec<CrateEvidence>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "atom-vibe-full-spine-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut fixture = Self {
            root,
            counter: 0,
            requirements: RequirementsRecord {
                purpose: String::new(),
                user_behaviors: Vec::new(),
                ui_decision: String::new(),
                persistence_decision: String::new(),
                external_boundaries: Vec::new(),
                execution_siting: String::new(),
                out_of_scope: Vec::new(),
                definition_of_done: Vec::new(),
                artifact: BuildArtifactRef {
                    path: String::new(),
                    sha256_hex: String::new(),
                    role: String::new(),
                },
            },
            blueprint: empty_blueprint(),
            crates: Vec::new(),
        };
        fixture.requirements = RequirementsRecord {
            purpose: "track inventory".to_string(),
            user_behaviors: vec!["add and remove inventory".to_string()],
            ui_decision: "native PMRE dashboard".to_string(),
            persistence_decision: "local durable state".to_string(),
            external_boundaries: vec!["none".to_string()],
            execution_siting: "local".to_string(),
            out_of_scope: vec!["cloud synchronization".to_string()],
            definition_of_done: vec!["inventory survives restart".to_string()],
            artifact: fixture.artifact("requirements", "complete requirements"),
        };
        fixture.blueprint = BlueprintRecord {
            version: 1,
            crates: vec![
                BlueprintCrate {
                    name: "state".to_string(),
                    responsibility: "own inventory state".to_string(),
                },
                BlueprintCrate {
                    name: "app".to_string(),
                    responsibility: "orchestrate UI messages".to_string(),
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
            blueprint_artifact: fixture.artifact("blueprint", "blueprint version one"),
            protocol_artifact: fixture.artifact("protocol", "InventoryChanged contract"),
            independent_review: fixture.review("blueprint-review"),
        };
        fixture.crates = ["state", "app"]
            .into_iter()
            .map(|name| {
                let source = fixture.source(name, "pub fn run() -> bool { true }\n");
                let check = fixture.command(&format!("check-{name}"));
                let test = fixture.command(&format!("test-{name}"));
                CrateEvidence {
                    crate_name: name.to_string(),
                    source_artifacts: vec![source],
                    cargo_check: check,
                    unit_test: test,
                }
            })
            .collect();
        fixture
    }

    fn artifact(&mut self, role: &str, content: &str) -> BuildArtifactRef {
        self.counter += 1;
        let path = format!("evidence/{:03}-{role}.txt", self.counter);
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

    fn command(&mut self, name: &str) -> CommandEvidence {
        CommandEvidence {
            name: name.to_string(),
            exit_code: 0,
            timed_out: false,
            warnings_denied: true,
            warning_count: 0,
            real_execution: true,
            stdout: self.artifact("command-stdout", &format!("{name} passed")),
            stderr: self.artifact("command-stderr", ""),
        }
    }

    fn review(&mut self, label: &str) -> IndependentReview {
        IndependentReview {
            author_identity_hash: "sha256:author".to_string(),
            reviewer_identity_hash: "sha256:independent-reviewer".to_string(),
            passed: true,
            findings: vec!["verified contract ownership".to_string()],
            findings_resolved: true,
            evidence: vec![self.artifact("independent-review", label)],
        }
    }

    fn base(&self, step: BuildStep) -> GateInput {
        let mut input = GateInput::new(step, &self.root);
        input.requirements = Some(self.requirements.clone());
        input.blueprint = Some(self.blueprint.clone());
        input.crates = self.crates.clone();
        input
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn empty_blueprint() -> BlueprintRecord {
    let empty = BuildArtifactRef {
        path: String::new(),
        sha256_hex: String::new(),
        role: String::new(),
    };
    BlueprintRecord {
        version: 0,
        crates: Vec::new(),
        message_contracts: Vec::new(),
        dependency_edges: Vec::new(),
        topological_order: Vec::new(),
        coupling_order: Vec::new(),
        blueprint_artifact: empty.clone(),
        protocol_artifact: empty,
        independent_review: IndependentReview {
            author_identity_hash: String::new(),
            reviewer_identity_hash: String::new(),
            passed: false,
            findings: Vec::new(),
            findings_resolved: false,
            evidence: Vec::new(),
        },
    }
}

#[test]
fn full_six_stage_build_survives_restart_between_every_gate() {
    let mut fixture = Fixture::new();
    let mut coordinator = BuildCoordinator::open(&fixture.root).unwrap();
    let build_id = coordinator
        .start_build("inventory-dashboard", true, 6)
        .unwrap();
    drop(coordinator);

    let mut intake = GateInput::new(BuildStep::Intake, &fixture.root);
    intake.requirements = Some(fixture.requirements.clone());
    pass_after_restart(&fixture, &build_id, intake, BuildStep::Blueprint);

    let mut blueprint = fixture.base(BuildStep::Blueprint);
    blueprint.crates.clear();
    pass_after_restart(&fixture, &build_id, blueprint, BuildStep::CrateBuild);

    let crate_build = fixture.base(BuildStep::CrateBuild);
    pass_after_restart(&fixture, &build_id, crate_build, BuildStep::CrateCouple);

    let mut couple = fixture.base(BuildStep::CrateCouple);
    couple.wirings = vec![WiringEvidence {
        contract_id: "state-to-app".to_string(),
        producer: "state".to_string(),
        consumer: "app".to_string(),
        message_type: "InventoryChanged".to_string(),
        status: WiringStatus::Pass,
        message_emitted: true,
        message_handled: true,
        direct_side_channel: false,
        deferred_reason: String::new(),
        evidence: vec![fixture.artifact("bus-flow", "message emitted and handled")],
    }];
    pass_after_restart(&fixture, &build_id, couple, BuildStep::BuildTest);

    let mut build_test = fixture.base(BuildStep::BuildTest);
    build_test.commands = ["cargo-check", "cargo-test", "cargo-clippy"]
        .into_iter()
        .map(|name| fixture.command(name))
        .collect();
    build_test.functional_cases = vec![FunctionalCaseEvidence {
        requirement: "inventory survives restart".to_string(),
        passed: true,
        smoke_only: false,
        real_workflow: true,
        bus_round_trip: true,
        evidence: vec![fixture.artifact("functional-workflow", "restart workflow passed")],
    }];
    build_test.implementation_review = Some(fixture.review("implementation-review"));
    pass_after_restart(&fixture, &build_id, build_test, BuildStep::LaunchProof);

    let mut launch = fixture.base(BuildStep::LaunchProof);
    launch.launch = Some(LaunchEvidence {
        process_started: true,
        process_still_running: true,
        panic_free: true,
        usable_screen_observed: true,
        screenshot: fixture.artifact("screenshot", "bitmap evidence"),
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
            evidence: vec![fixture.artifact("ui-round-trip", "input changed rendered state")],
        },
        definition_of_done_satisfied: true,
        definition_of_done_evidence: vec![
            fixture.artifact("definition-of-done", "restart verified")
        ],
        completion_enforcer_clean: true,
    });
    let mut coordinator = BuildCoordinator::open(&fixture.root).unwrap();
    let decision = coordinator.evaluate(&build_id, &launch).unwrap();
    assert!(matches!(decision, BuildPlannerDecision::Advance(_)));
    assert_eq!(coordinator.routes().last().unwrap().route.len(), 4);
    drop(coordinator);

    let final_coordinator = BuildCoordinator::open(&fixture.root).unwrap();
    let ledger = final_coordinator.get(&build_id).unwrap().ledger();
    assert_eq!(ledger.status, BuildRunStatus::Complete);
    assert_eq!(ledger.step_outputs.len(), 6);
    assert!(ledger.launch_proof.is_some());
    assert_eq!(ledger.revision, 7);
}

fn pass_after_restart(
    fixture: &Fixture,
    build_id: &str,
    input: GateInput,
    expected_step: BuildStep,
) {
    let mut coordinator = BuildCoordinator::open(&fixture.root).unwrap();
    let decision = coordinator.evaluate(build_id, &input).unwrap();
    assert!(matches!(decision, BuildPlannerDecision::Advance(_)));
    assert_eq!(coordinator.routes().last().unwrap().route.len(), 4);
    assert_eq!(
        coordinator.get(build_id).unwrap().ledger().current_step,
        expected_step
    );
}
