use crate::{
    BuildLedger, BuildLedgerStore, BuildPlanner, BuildPlannerDecision, BuildPlannerError,
    BuildRunStatus,
};
use atom_vibe_build_gates::GateInput;
use atom_vibe_build_protocol::BuildPlannerEvent;
use math_atoms_bus::{BusMessageKind, EnvelopeId, SpiderwebBus};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannerBusRoute {
    pub build_id: String,
    pub event: String,
    pub terminal: EnvelopeId,
    pub route: Vec<EnvelopeId>,
}

pub struct BuildCoordinator {
    store: BuildLedgerStore,
    planners: HashMap<String, BuildPlanner>,
    bus: SpiderwebBus,
    routes: Vec<PlannerBusRoute>,
}

impl BuildCoordinator {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, BuildPlannerError> {
        let store = BuildLedgerStore::open(root.into())
            .map_err(|error| BuildPlannerError::Store(error.to_string()))?;
        let mut planners = HashMap::new();
        for ledger in store
            .list()
            .map_err(|error| BuildPlannerError::Store(error.to_string()))?
        {
            let build_id = ledger.build_id.clone();
            planners.insert(build_id, BuildPlanner::from_ledger(ledger)?);
        }
        Ok(Self {
            store,
            planners,
            bus: SpiderwebBus::new(),
            routes: Vec::new(),
        })
    }

    pub fn start_build(
        &mut self,
        project_id: impl Into<String>,
        autonomous_correction: bool,
        retry_limit: u8,
    ) -> Result<String, BuildPlannerError> {
        let planner = BuildPlanner::new(project_id, autonomous_correction, retry_limit)?;
        let build_id = planner.ledger().build_id.clone();
        self.store
            .save(planner.ledger())
            .map_err(|error| BuildPlannerError::Store(error.to_string()))?;
        let event = planner.start_event();
        self.emit(&build_id, &event, &[], None)?;
        self.planners.insert(build_id.clone(), planner);
        Ok(build_id)
    }

    pub fn evaluate(
        &mut self,
        build_id: &str,
        input: &GateInput,
    ) -> Result<BuildPlannerDecision, BuildPlannerError> {
        let mut candidate = self
            .planners
            .get(build_id)
            .cloned()
            .ok_or_else(|| BuildPlannerError::BuildNotFound(build_id.to_string()))?;
        let decision = candidate.evaluate_and_handle(input)?;
        self.store
            .save(candidate.ledger())
            .map_err(|error| BuildPlannerError::Store(error.to_string()))?;
        let evidence_ids = decision_evidence_ids(candidate.ledger(), &decision);
        self.emit(
            build_id,
            decision.event(),
            &evidence_ids,
            Some(decision.label()),
        )?;
        self.planners.insert(build_id.to_string(), candidate);
        Ok(decision)
    }

    pub fn get(&self, build_id: &str) -> Option<&BuildPlanner> {
        self.planners.get(build_id)
    }

    pub fn ledgers(&self) -> Vec<BuildLedger> {
        let mut ledgers = self
            .planners
            .values()
            .map(|planner| planner.ledger().clone())
            .collect::<Vec<_>>();
        ledgers.sort_by_key(|ledger| std::cmp::Reverse(ledger.updated_at_unix_ms));
        ledgers
    }

    pub fn active_builds(&self) -> Vec<String> {
        let mut builds = self
            .planners
            .values()
            .filter(|planner| planner.ledger().status == BuildRunStatus::Active)
            .map(|planner| planner.ledger().build_id.clone())
            .collect::<Vec<_>>();
        builds.sort();
        builds
    }

    pub fn bus(&self) -> &SpiderwebBus {
        &self.bus
    }

    pub fn routes(&self) -> &[PlannerBusRoute] {
        &self.routes
    }

    fn emit(
        &mut self,
        build_id: &str,
        event: &BuildPlannerEvent,
        evidence_ids: &[String],
        decision: Option<&str>,
    ) -> Result<(), BuildPlannerError> {
        let start = matches!(event, BuildPlannerEvent::BuildStarted { .. });
        let label = decision.unwrap_or("build_started");
        let body = planner_event_body(event);
        let ingress = self.bus.l0_transport(
            if start {
                BusMessageKind::IntentIngress
            } else {
                BusMessageKind::WorkPacketExecuted
            },
            if start { "operator" } else { "build-gate" },
            "atom-build-planner",
            &body,
        );
        let blocked = matches!(label, "hard_halt" | "autonomous_retry_requested");
        let message = self.bus.l1_message(
            ingress,
            if blocked {
                BusMessageKind::ProofBlocked
            } else if start {
                BusMessageKind::WorkPlanCreated
            } else {
                BusMessageKind::ProofCaptured
            },
            "atom-build-planner",
            "build-ledger-store",
            label,
        );
        let flow = self.bus.l2_flow(
            message,
            if blocked {
                BusMessageKind::StoreBlocked
            } else {
                BusMessageKind::StoreObserved
            },
            "build-ledger-store",
            "build-skill-router",
            &body,
            evidence_ids,
        );
        let terminal = self.bus.l3_orchestrate(
            flow,
            if blocked {
                BusMessageKind::ProofBlocked
            } else if matches!(label, "advance") {
                BusMessageKind::WorkPlanCompleted
            } else {
                BusMessageKind::WorkPlanCreated
            },
            "build-skill-router",
            "atom-vibe-runtime",
            &body,
            evidence_ids,
        );
        if !self.bus.route_contains_all_layers(terminal) {
            return Err(BuildPlannerError::IncompleteSpiderwebRoute);
        }
        self.routes.push(PlannerBusRoute {
            build_id: build_id.to_string(),
            event: label.to_string(),
            terminal,
            route: self
                .bus
                .route_for(terminal)
                .iter()
                .map(|envelope| envelope.id)
                .collect(),
        });
        Ok(())
    }
}

fn decision_evidence_ids(ledger: &BuildLedger, decision: &BuildPlannerDecision) -> Vec<String> {
    let artifacts = match decision {
        BuildPlannerDecision::Advance(_) => ledger
            .step_outputs
            .last()
            .map(|output| output.artifacts.as_slice()),
        BuildPlannerDecision::HardHalt(_) | BuildPlannerDecision::AutonomousRetry(_) => ledger
            .retry_ledger
            .last()
            .map(|record| record.artifacts.as_slice()),
        BuildPlannerDecision::Deferred(_) => None,
    };
    artifacts
        .unwrap_or_default()
        .iter()
        .map(|artifact| format!("artifact:{}", artifact.path))
        .collect()
}

fn planner_event_body(event: &BuildPlannerEvent) -> String {
    match event {
        BuildPlannerEvent::BuildStarted {
            project_id,
            released_skill,
            ..
        } => format!("build started for {project_id}; released {released_skill}"),
        BuildPlannerEvent::StepAdvanced {
            completed_step,
            released_skill,
            ..
        } => format!(
            "{} passed; released {}",
            completed_step.as_str(),
            released_skill.as_deref().unwrap_or("build-complete")
        ),
        BuildPlannerEvent::DeferredDebtRecorded { step, reason, .. } => {
            format!("{} deferred debt recorded: {reason}", step.as_str())
        }
        BuildPlannerEvent::HardHalted {
            step,
            error_class,
            summary,
            ..
        } => format!("{} hard halted for {error_class}: {summary}", step.as_str()),
        BuildPlannerEvent::AutonomousRetryRequested {
            step,
            attempt,
            attempt_limit,
            summary,
            ..
        } => format!(
            "{} correction {attempt}/{attempt_limit} requested: {summary}",
            step.as_str()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_vibe_build_gates::RequirementsRecord;
    use atom_vibe_build_protocol::{BuildArtifactRef, BuildPlannerEvent, BuildStep};
    use math_atoms_hash::sha256_hex;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-coordinator-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn artifact(root: &Path, name: &str, text: &str) -> BuildArtifactRef {
        let path = format!("evidence/{name}.txt");
        let absolute = root.join(&path);
        fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        fs::write(&absolute, text).unwrap();
        BuildArtifactRef::new(path, sha256_hex(text.as_bytes()), name).unwrap()
    }

    fn valid_intake(root: &Path) -> GateInput {
        let mut input = GateInput::new(BuildStep::Intake, root);
        input.requirements = Some(RequirementsRecord {
            purpose: "track inventory".to_string(),
            user_behaviors: vec!["add items".to_string()],
            ui_decision: "native PMRE".to_string(),
            persistence_decision: "local durable store".to_string(),
            external_boundaries: vec!["none".to_string()],
            execution_siting: "local".to_string(),
            out_of_scope: vec!["cloud sync".to_string()],
            definition_of_done: vec!["item survives restart".to_string()],
            artifact: artifact(root, "requirements", "complete requirements"),
        });
        input
    }

    #[test]
    fn start_and_gate_pass_persist_and_traverse_all_layers() {
        let root = root("pass");
        let mut coordinator = BuildCoordinator::open(&root).unwrap();
        let build_id = coordinator.start_build("inventory", true, 6).unwrap();
        assert_eq!(coordinator.routes()[0].route.len(), 4);
        assert!(coordinator
            .bus()
            .route_contains_all_layers(coordinator.routes()[0].terminal));

        let decision = coordinator
            .evaluate(&build_id, &valid_intake(&root))
            .unwrap();
        assert!(matches!(decision, BuildPlannerDecision::Advance(_)));
        assert_eq!(
            coordinator.get(&build_id).unwrap().ledger().current_step,
            BuildStep::Blueprint
        );
        assert_eq!(coordinator.routes()[1].route.len(), 4);

        let restarted = BuildCoordinator::open(&root).unwrap();
        assert_eq!(
            restarted.get(&build_id).unwrap().ledger().current_step,
            BuildStep::Blueprint
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn model_narrative_cannot_advance_and_six_corrections_are_bounded() {
        let root = root("retry");
        let mut coordinator = BuildCoordinator::open(&root).unwrap();
        let build_id = coordinator.start_build("inventory", true, 6).unwrap();
        let invalid = GateInput::new(BuildStep::Intake, &root);
        for expected_attempt in 1..=6 {
            let decision = coordinator.evaluate(&build_id, &invalid).unwrap();
            match decision {
                BuildPlannerDecision::AutonomousRetry(
                    BuildPlannerEvent::AutonomousRetryRequested { attempt, .. },
                ) => assert_eq!(attempt, expected_attempt),
                other => panic!("expected retry, got {other:?}"),
            }
            assert_eq!(
                coordinator.get(&build_id).unwrap().ledger().current_step,
                BuildStep::Intake
            );
        }
        let final_decision = coordinator.evaluate(&build_id, &invalid).unwrap();
        assert!(matches!(
            final_decision,
            BuildPlannerDecision::HardHalt(BuildPlannerEvent::HardHalted { .. })
        ));
        assert_eq!(
            coordinator.get(&build_id).unwrap().ledger().status,
            BuildRunStatus::Halted
        );
        assert!(coordinator.bus().backpressure().len() >= 7);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wrong_step_is_rejected_without_mutating_or_persisting() {
        let root = root("wrong-step");
        let mut coordinator = BuildCoordinator::open(&root).unwrap();
        let build_id = coordinator.start_build("inventory", true, 6).unwrap();
        let before = coordinator.get(&build_id).unwrap().ledger().clone();
        let wrong = GateInput::new(BuildStep::Blueprint, &root);
        assert!(matches!(
            coordinator.evaluate(&build_id, &wrong),
            Err(BuildPlannerError::WrongStep { .. })
        ));
        assert_eq!(coordinator.get(&build_id).unwrap().ledger(), &before);
        assert_eq!(coordinator.routes().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
