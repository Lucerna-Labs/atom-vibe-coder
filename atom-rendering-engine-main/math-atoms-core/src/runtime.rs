use crate::domain::{atom_by_key, atoms, mission, recipes, Recipe};
use crate::provider::{
    provider_output_hash, PreparedProviderCall, ProviderConfig, ProviderError,
    ProviderExecutionOutput, ProviderWireFormat,
};
use math_atoms_bus::{BusMessageKind, EnvelopeId, SpiderwebBus};
use math_atoms_graph::{Evidence, WikiGraph};
use math_atoms_hash::sha256_file;
use math_atoms_learning::{
    effective_records, LearningOutcome, LearningRecord, LearningStore, DEFAULT_GRAPH_MEMORY_LIMIT,
};
use math_atoms_proof::{ProofRecord, ProofStore};
use math_atoms_secrets::redact_sensitive_text;
use math_atoms_work::verify_work_plan_evidence;
use std::collections::HashSet;
use std::fs;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeStatus {
    Draft,
    ProviderPending,
    VerificationPending,
    Proven,
    Blocked,
    DriftFlagged,
}

impl RuntimeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::ProviderPending => "provider pending",
            Self::VerificationPending => "verification pending",
            Self::Proven => "proven",
            Self::Blocked => "blocked",
            Self::DriftFlagged => "drift flagged",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeState {
    pub selected_recipe: String,
    pub selected_atoms: Vec<String>,
    pub status: RuntimeStatus,
    pub proof_count: u32,
    pub drift_count: u32,
    pub evidence: Vec<Evidence>,
    pub blockers: Vec<String>,
    pub last_provider_call: Option<PreparedProviderCall>,
    pub last_provider_output_artifact: String,
    pub last_provider_output_hash: String,
    pub last_provider_output_len: usize,
    pub last_work_plan_id: String,
    pub last_work_plan_manifest: String,
    pub last_work_packet_count: usize,
    pub last_route: Vec<EnvelopeId>,
}

#[derive(Clone, Debug)]
pub struct ProofRun {
    pub recipe_id: String,
    pub atom_keys: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub provider_call: Option<PreparedProviderCall>,
    pub blockers: Vec<String>,
    pub proof_envelope: EnvelopeId,
    pub status: RuntimeStatus,
}

#[derive(Clone, Debug)]
pub struct ProviderExecutionTask {
    pub call: PreparedProviderCall,
    pub route: Vec<EnvelopeId>,
}

#[derive(Clone, Debug)]
pub struct MathAtomsRuntime {
    bus: SpiderwebBus,
    graph: WikiGraph,
    provider: ProviderConfig,
    state: RuntimeState,
}

impl MathAtomsRuntime {
    pub fn new(provider: ProviderConfig) -> Self {
        Self::with_stores(
            provider,
            ProofStore::new(ProofStore::default_path()),
            LearningStore::new(LearningStore::default_path()),
        )
    }

    pub fn with_proof_store(provider: ProviderConfig, proof_store: ProofStore) -> Self {
        let learning_store = LearningStore::beside(proof_store.path());
        Self::with_stores(provider, proof_store, learning_store)
    }

    pub fn with_stores(
        provider: ProviderConfig,
        proof_store: ProofStore,
        learning_store: LearningStore,
    ) -> Self {
        let mut graph = WikiGraph::from_default_dirs();
        let mut store_errors = Vec::new();
        match proof_store.read_records() {
            Ok(records) => {
                graph.add_proof_records(&records);
            }
            Err(error) => store_errors.push(format!(
                "Proof store read failed at {}: {error}",
                proof_store.path().display()
            )),
        }
        match learning_store.read_records() {
            Ok(records) => {
                graph.add_learning_records(&effective_records(&records, DEFAULT_GRAPH_MEMORY_LIMIT))
            }
            Err(error) => store_errors.push(format!(
                "Learning store read failed at {}: {error}",
                learning_store.path().display()
            )),
        }
        let mut runtime = Self::with_graph(provider, graph);
        for reason in store_errors {
            runtime.mark_startup_store_blocked(&reason);
        }
        runtime
    }

    pub fn with_graph(provider: ProviderConfig, graph: WikiGraph) -> Self {
        Self {
            bus: SpiderwebBus::new(),
            graph,
            provider,
            state: RuntimeState {
                selected_recipe: "native-atom-renderer".to_string(),
                selected_atoms: Vec::new(),
                status: RuntimeStatus::Draft,
                proof_count: 0,
                drift_count: 0,
                evidence: Vec::new(),
                blockers: Vec::new(),
                last_provider_call: None,
                last_provider_output_artifact: String::new(),
                last_provider_output_hash: String::new(),
                last_provider_output_len: 0,
                last_work_plan_id: String::new(),
                last_work_plan_manifest: String::new(),
                last_work_packet_count: 0,
                last_route: Vec::new(),
            },
        }
    }

    pub fn from_process_env() -> Self {
        Self::new(ProviderConfig::from_process_env())
    }

    pub fn bus(&self) -> &SpiderwebBus {
        &self.bus
    }

    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    pub fn provider(&self) -> &ProviderConfig {
        &self.provider
    }

    pub fn set_provider(&mut self, provider: ProviderConfig) {
        self.provider = provider;
        self.state.status = RuntimeStatus::Draft;
        self.state.selected_recipe = "native-atom-renderer".to_string();
        self.state.selected_atoms.clear();
        self.state.evidence.clear();
        self.state.blockers.clear();
        self.state.last_provider_call = None;
        self.state.last_provider_output_artifact.clear();
        self.state.last_provider_output_hash.clear();
        self.state.last_provider_output_len = 0;
        self.state.last_work_plan_id.clear();
        self.state.last_work_plan_manifest.clear();
        self.state.last_work_packet_count = 0;
        self.state.last_route.clear();
    }

    pub fn run_intent(&mut self, intent: &str) -> ProofRun {
        let provider_route = provider_route_required(intent);
        let l0 = self.bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator",
            "math-atoms-runtime",
            intent,
        );
        let intent_atoms = classify_intent(intent);
        let atom_body = intent_atoms.join(",");
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::IntentClassified,
            "classifier",
            "wiki-graph",
            &atom_body,
        );
        let evidence = self.graph.retrieve(intent, &intent_atoms, 8);
        let evidence_ids: Vec<String> = evidence.iter().map(|item| item.node_id.clone()).collect();
        let learning_hits = evidence
            .iter()
            .filter(|item| item.node_id.starts_with("learning:"))
            .count();
        let evidence_body = format!(
            "graph evidence ranked from atom and recipe relationships; {learning_hits} durable learning records preloaded"
        );
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::EvidenceRetrieved,
            "wiki-graph",
            "recipe-selector",
            &evidence_body,
            &evidence_ids,
        );
        let recipe = select_recipe(
            intent,
            &intent_atoms,
            &evidence,
            self.provider.is_ready(),
            provider_route,
        );
        let atom_stack = recipe_stack(recipe);
        let provider_result = if recipe.requires_provider || provider_route {
            self.provider
                .prepare_call_with_atoms(intent, recipe.id, &atom_stack, &evidence)
        } else {
            Ok(PreparedProviderCall {
                endpoint: String::new(),
                model: String::new(),
                api_key_env: String::new(),
                auth_header: String::new(),
                auth_scheme: String::new(),
                wire_format: ProviderWireFormat::OpenAiResponses,
                response_key: String::new(),
                body: String::new(),
                work_plan: None,
                evidence_context: String::new(),
                body_template: String::new(),
                request_timeout_seconds: 120,
                plan_timeout_seconds: 11_520,
                verification_attempt_limit: 8,
                verification_timeout_seconds: 180,
                credential_scope_hash: String::new(),
            })
        };

        let mut blockers = Vec::new();
        let mut recipe_parent = l2;
        let provider_call = match provider_result {
            Ok(call) if !call.body.is_empty() => {
                recipe_parent = self.bus.l2_flow(
                    l2,
                    BusMessageKind::ProviderPrepared,
                    "provider-adapter",
                    "model",
                    "provider request prepared from graph evidence",
                    &evidence_ids,
                );
                if let Some(plan) = &call.work_plan {
                    recipe_parent = self.bus.l2_flow(
                        recipe_parent,
                        BusMessageKind::WorkPlanCreated,
                        "work-planner",
                        "provider-worker",
                        &format!(
                            "{} base packets prepared; file packets expand from the strict manifest",
                            plan.packets.len()
                        ),
                        &evidence_ids,
                    );
                }
                Some(call)
            }
            Ok(_) => None,
            Err(ProviderError::MissingApiKey { env }) => {
                blockers.push(format!("Missing provider credential in {env}"));
                recipe_parent = self.bus.l2_flow(
                    l2,
                    BusMessageKind::ProviderBlocked,
                    "provider-adapter",
                    "operator",
                    "provider credential missing; route failed closed",
                    &evidence_ids,
                );
                None
            }
            Err(error) => {
                blockers.push(format!("Provider setup failed: {error:?}"));
                None
            }
        };

        if !evidence
            .iter()
            .any(|item| item.node_id == "mission:production-app-build")
        {
            blockers.push("Production app mission evidence was not retrieved".to_string());
        }
        if !self
            .graph
            .has_relationship_path(recipe.id, "mission:production-app-build", 6)
        {
            blockers.push(format!(
                "{} is not graph-linked to the production app mission",
                recipe.id
            ));
        }
        if stack_quality(&atom_stack, recipe.atoms) < 0 {
            blockers.push(format!("{} has an invalid canonical atom stack", recipe.id));
        }

        let l3 = self.bus.l3_orchestrate(
            recipe_parent,
            BusMessageKind::RecipeSelected,
            "recipe-selector",
            "proof-loop",
            recipe.id,
            &evidence_ids,
        );
        if !self.bus.route_contains_all_layers(l3) {
            blockers.push("Spiderweb Bus route did not touch all L0-L3 layers".to_string());
        }
        let provider_pending = blockers.is_empty() && provider_call.is_some();
        let status = if !blockers.is_empty() {
            RuntimeStatus::Blocked
        } else if provider_pending {
            RuntimeStatus::ProviderPending
        } else {
            RuntimeStatus::Proven
        };
        let proof_kind = if !blockers.is_empty() {
            BusMessageKind::ProofBlocked
        } else if provider_pending {
            BusMessageKind::ProofPending
        } else {
            BusMessageKind::ProofCaptured
        };
        let proof_body = if !blockers.is_empty() {
            blockers.join("; ")
        } else if provider_pending {
            format!(
                "{} selected for {}; provider execution required before proof can pass",
                recipe.name,
                mission().readiness_floor
            )
        } else {
            format!(
                "{} selected for {} with {} evidence nodes",
                recipe.name,
                mission().readiness_floor,
                evidence.len()
            )
        };
        let proof = self.bus.l3_orchestrate(
            l3,
            proof_kind,
            "proof-loop",
            "artifact-state",
            &proof_body,
            &evidence_ids,
        );

        self.state.selected_recipe = recipe.id.to_string();
        self.state.selected_atoms = atom_stack.clone();
        self.state.status = status;
        self.state.evidence = evidence.clone();
        self.state.blockers = blockers.clone();
        self.state.last_provider_call = provider_call.clone();
        self.state.last_provider_output_artifact.clear();
        self.state.last_provider_output_hash.clear();
        self.state.last_provider_output_len = 0;
        self.state.last_work_plan_id = provider_call
            .as_ref()
            .and_then(|call| call.work_plan.as_ref())
            .map(|plan| plan.id.clone())
            .unwrap_or_default();
        self.state.last_work_plan_manifest.clear();
        self.state.last_work_packet_count = 0;
        self.state.last_route = self.bus.route_for(proof).iter().map(|env| env.id).collect();
        if status == RuntimeStatus::Proven {
            self.state.proof_count += 1;
        }

        ProofRun {
            recipe_id: recipe.id.to_string(),
            atom_keys: atom_stack,
            evidence,
            provider_call,
            blockers,
            proof_envelope: proof,
            status,
        }
    }

    pub fn mark_drift(&mut self, reason: &str) {
        self.state.drift_count += 1;
        self.state.status = RuntimeStatus::DriftFlagged;
        self.bus.l3_orchestrate(
            self.state.last_route.last().copied().unwrap_or(0),
            BusMessageKind::DriftFlagged,
            "operator",
            "proof-loop",
            reason,
            &[],
        );
    }

    pub fn schedule_provider_execution(&mut self) -> Option<ProviderExecutionTask> {
        let Some(call) = self.state.last_provider_call.clone() else {
            self.mark_provider_blocked(
                "No prepared provider call. Run an intent that requests provider/model work first.",
            );
            return None;
        };
        if self.state.status != RuntimeStatus::ProviderPending {
            self.mark_provider_blocked(
                "Provider execution requires a provider pending Spiderweb route",
            );
            return None;
        }
        let evidence_ids = self.provider_evidence_ids();
        let parent = self.state.last_route.last().copied();
        let l0 = self.bus.l0_transport_from(
            parent,
            BusMessageKind::ProviderExecutionRequested,
            "proof-loop",
            "provider-adapter",
            "provider execution requested from pending proof route",
        );
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::ProviderExecutionRequested,
            "provider-adapter",
            "model-worker",
            &format!("{} via {}", call.model, call.endpoint),
        );
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::ProviderExecutionScheduled,
            "provider-adapter",
            "model-worker",
            "provider worker scheduled with graph evidence payload",
            &evidence_ids,
        );
        let l3 = self.bus.l3_orchestrate(
            l2,
            BusMessageKind::ProviderExecutionScheduled,
            "proof-loop",
            "provider-worker",
            "provider execution lifted onto Spiderweb orchestration route",
            &evidence_ids,
        );
        self.state.last_route = self.bus.route_for(l3).iter().map(|env| env.id).collect();
        Some(ProviderExecutionTask {
            call,
            route: self.state.last_route.clone(),
        })
    }

    pub fn mark_provider_executed(
        &mut self,
        output_artifact: &str,
        output_hash: &str,
        output_len: usize,
    ) {
        self.mark_provider_execution_report(
            output_artifact,
            output_hash,
            output_len,
            &ProviderExecutionOutput {
                text: String::new(),
                work_plan_id: String::new(),
                work_plan_manifest: String::new(),
                packet_ids: Vec::new(),
                executed_packets: 1,
                resumed_packets: 0,
                candidate_verification: None,
            },
        );
    }

    pub fn mark_provider_execution_report(
        &mut self,
        output_artifact: &str,
        output_hash: &str,
        output_len: usize,
        report: &ProviderExecutionOutput,
    ) -> bool {
        if let Err(reason) =
            self.verify_provider_execution_report(output_artifact, output_hash, output_len, report)
        {
            self.mark_provider_blocked(&format!("provider report verification failed: {reason}"));
            return false;
        }
        self.state.status = RuntimeStatus::VerificationPending;
        self.state.last_provider_output_artifact = output_artifact.to_string();
        self.state.last_provider_output_hash = output_hash.to_string();
        self.state.last_provider_output_len = output_len;
        self.state.last_work_plan_id = report.work_plan_id.clone();
        self.state.last_work_plan_manifest = report.work_plan_manifest.clone();
        self.state.last_work_packet_count = report.packet_ids.len();
        let evidence_ids = self.provider_evidence_ids();
        let model = self
            .state
            .last_provider_call
            .as_ref()
            .map(|call| call.model.as_str())
            .unwrap_or("provider");
        let body = format!("{model} executed output {output_hash} ({output_len} bytes)");
        let parent = self.state.last_route.last().copied();
        let l0 = self.bus.l0_transport_from(
            parent,
            BusMessageKind::ProviderExecuted,
            "model-worker",
            "provider-adapter",
            &body,
        );
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::ProviderExecuted,
            "provider-adapter",
            "proof-loop",
            &body,
        );
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::ProviderExecuted,
            "provider-adapter",
            "proof-loop",
            &body,
            &evidence_ids,
        );
        let mut packet_parent = l2;
        for (index, packet_id) in report.packet_ids.iter().enumerate() {
            packet_parent = self.bus.l2_flow(
                packet_parent,
                BusMessageKind::WorkPacketExecuted,
                "provider-worker",
                "work-proof",
                &format!(
                    "packet {}/{} completed: {packet_id}",
                    index + 1,
                    report.packet_ids.len()
                ),
                &evidence_ids,
            );
        }
        let completion_parent = if report.packet_ids.is_empty() {
            packet_parent
        } else {
            self.bus.l3_orchestrate(
                packet_parent,
                BusMessageKind::WorkPlanCompleted,
                "work-proof",
                "proof-loop",
                &format!(
                    "{} packets complete; {} executed, {} resumed",
                    report.packet_ids.len(),
                    report.executed_packets,
                    report.resumed_packets
                ),
                &evidence_ids,
            )
        };
        let l3 = self.bus.l3_orchestrate(
            completion_parent,
            BusMessageKind::ProviderExecuted,
            "proof-loop",
            "artifact-state",
            "provider worker completed through Spiderweb route",
            &evidence_ids,
        );
        let proof = self.bus.l3_orchestrate(
            l3,
            BusMessageKind::ProofPending,
            "proof-loop",
            "artifact-state",
            "provider work evidence verified; executable product harness is still required",
            &evidence_ids,
        );
        self.state.last_route = self.bus.route_for(proof).iter().map(|env| env.id).collect();
        true
    }

    fn verify_provider_execution_report(
        &self,
        output_artifact: &str,
        output_hash: &str,
        output_len: usize,
        report: &ProviderExecutionOutput,
    ) -> Result<(), String> {
        if self.state.status != RuntimeStatus::ProviderPending {
            return Err("runtime is not provider pending".to_string());
        }
        let call = self
            .state
            .last_provider_call
            .as_ref()
            .ok_or_else(|| "prepared provider call is missing".to_string())?;
        let prepared_plan = call
            .work_plan
            .as_ref()
            .ok_or_else(|| "prepared meticulous work plan is missing".to_string())?;
        if report.work_plan_id != prepared_plan.id || report.work_plan_manifest.trim().is_empty() {
            return Err("reported work plan does not match the prepared plan".to_string());
        }
        let verified = verify_work_plan_evidence(
            &report.work_plan_manifest,
            &report.work_plan_id,
            report.packet_ids.len(),
        )
        .map_err(|error| error.to_string())?;
        let total = report
            .executed_packets
            .checked_add(report.resumed_packets)
            .ok_or_else(|| "packet accounting overflowed".to_string())?;
        let unique = report.packet_ids.iter().collect::<HashSet<_>>();
        if total != report.packet_ids.len()
            || unique.len() != report.packet_ids.len()
            || report.packet_ids != verified.packet_ids
            || verified.model != call.model
        {
            return Err("packet IDs, accounting, or model do not match evidence".to_string());
        }
        if output_artifact.trim().is_empty()
            || output_len == 0
            || report.text.len() != output_len
            || provider_output_hash(&report.text) != output_hash
        {
            return Err("provider output claim is empty or inconsistent".to_string());
        }
        let candidate = report
            .candidate_verification
            .as_ref()
            .ok_or_else(|| "real candidate verification evidence is missing".to_string())?;
        if candidate.bundle_hash != output_hash {
            return Err("candidate bundle does not match provider output".to_string());
        }
        let verified_candidate = math_atoms_verification::verify_candidate_evidence(
            &candidate.manifest_path,
            &candidate.manifest_hash,
            &report.work_plan_id,
            &candidate.bundle_hash,
        )
        .map_err(|error| error.to_string())?;
        if verified_candidate.attempts != candidate.attempts
            || verified_candidate.repairs != candidate.repairs
        {
            return Err("candidate verification accounting does not match evidence".to_string());
        }
        let metadata = fs::metadata(output_artifact).map_err(|error| error.to_string())?;
        let actual_hash = sha256_file(output_artifact).map_err(|error| error.to_string())?;
        if metadata.len() != output_len as u64 || actual_hash != output_hash {
            return Err("provider output artifact does not recompute".to_string());
        }
        Ok(())
    }

    pub fn mark_provider_blocked(&mut self, reason: &str) {
        let reason = redact_sensitive_text(reason);
        self.state.status = RuntimeStatus::Blocked;
        if !self.state.blockers.iter().any(|item| item == &reason) {
            self.state.blockers.push(reason.clone());
        }
        let evidence_ids = self.provider_evidence_ids();
        let parent = self.state.last_route.last().copied();
        let l0 = self.bus.l0_transport_from(
            parent,
            BusMessageKind::ProviderBlocked,
            "model-worker",
            "provider-adapter",
            &reason,
        );
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::ProviderBlocked,
            "provider-adapter",
            "proof-loop",
            &reason,
        );
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::ProviderBlocked,
            "provider-adapter",
            "proof-loop",
            &reason,
            &evidence_ids,
        );
        let l3 = self.bus.l3_orchestrate(
            l2,
            BusMessageKind::ProviderBlocked,
            "proof-loop",
            "artifact-state",
            &reason,
            &evidence_ids,
        );
        self.state.last_route = self.bus.route_for(l3).iter().map(|env| env.id).collect();
    }

    pub fn mark_store_blocked(&mut self, reason: &str) {
        let reason = redact_sensitive_text(reason);
        self.state.status = RuntimeStatus::Blocked;
        if !self.state.blockers.iter().any(|item| item == &reason) {
            self.state.blockers.push(reason.clone());
        }
        let evidence_ids: Vec<String> = self
            .state
            .evidence
            .iter()
            .map(|item| item.node_id.clone())
            .collect();
        self.bus.l3_orchestrate(
            self.state.last_route.last().copied().unwrap_or(0),
            BusMessageKind::StoreBlocked,
            "proof-store",
            "proof-loop",
            &reason,
            &evidence_ids,
        );
    }

    fn mark_startup_store_blocked(&mut self, reason: &str) {
        self.state.status = RuntimeStatus::Blocked;
        if !self.state.blockers.iter().any(|item| item == reason) {
            self.state.blockers.push(reason.to_string());
        }
        let l0 = self.bus.l0_transport(
            BusMessageKind::StoreBlocked,
            "proof-store",
            "math-atoms-runtime",
            reason,
        );
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::StoreBlocked,
            "math-atoms-runtime",
            "wiki-graph",
            "persistent proof records rejected",
        );
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::StoreBlocked,
            "wiki-graph",
            "proof-loop",
            "startup proof evidence blocked before retrieval",
            &[],
        );
        let l3 = self.bus.l3_orchestrate(
            l2,
            BusMessageKind::StoreBlocked,
            "proof-loop",
            "artifact-state",
            reason,
            &[],
        );
        self.state.last_route = self.bus.route_for(l3).iter().map(|env| env.id).collect();
    }

    pub fn learn_proof_record(&mut self, record: &ProofRecord) {
        if record.status == RuntimeStatus::Proven.as_str() {
            self.graph.add_proof_record(record);
            self.bus.l3_orchestrate(
                self.state.last_route.last().copied().unwrap_or(0),
                BusMessageKind::StoreLearned,
                "proof-store",
                "wiki-graph",
                "stored proof record loaded into graph evidence",
                &[],
            );
        } else {
            self.bus.l3_orchestrate(
                self.state.last_route.last().copied().unwrap_or(0),
                BusMessageKind::StoreObserved,
                "proof-store",
                "proof-loop",
                "stored non-proven run observed but not promoted to graph evidence",
                &[],
            );
        }
    }

    pub fn learn_learning_record(&mut self, record: &LearningRecord) {
        self.graph.add_learning_record(record);
        let node_id = record.node_id();
        let evidence_ids = vec![node_id];
        let parent = self.state.last_route.last().copied();
        let l0 = self.bus.l0_transport_from(
            parent,
            BusMessageKind::LearningObserved,
            &record.source,
            "learning-store",
            &record.title(),
        );
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::LearningPersisted,
            "learning-store",
            "wiki-graph",
            "validated learning event persisted and read back",
        );
        let body = if record.outcome == LearningOutcome::Succeeded {
            "gate-passing result promoted as reusable graph evidence"
        } else {
            "failure joined as correction evidence without proof promotion"
        };
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::LearningApplied,
            "wiki-graph",
            "recipe-selector",
            body,
            &evidence_ids,
        );
        let l3 = self.bus.l3_orchestrate(
            l2,
            BusMessageKind::LearningApplied,
            "proof-loop",
            "artifact-state",
            body,
            &evidence_ids,
        );
        self.state.last_route = self.bus.route_for(l3).iter().map(|env| env.id).collect();
    }

    fn provider_evidence_ids(&self) -> Vec<String> {
        self.state
            .evidence
            .iter()
            .map(|item| item.node_id.clone())
            .collect()
    }
}

fn classify_intent(intent: &str) -> Vec<String> {
    let tokens = intent_tokens(intent);
    let provider_route = provider_route_required_from_tokens(&tokens, &[]);
    let mut scored: Vec<(&str, i32)> = atoms()
        .iter()
        .map(|atom| {
            let score = atom
                .keywords
                .iter()
                .filter(|keyword| tokens_match_keyword(&tokens, keyword))
                .count() as i32;
            (atom.key, score)
        })
        .filter(|(_, score)| *score > 0)
        .collect();
    scored.sort_by(|(a_key, a_score), (b_key, b_score)| {
        b_score.cmp(a_score).then_with(|| {
            atom_by_key(a_key)
                .unwrap()
                .id
                .cmp(&atom_by_key(b_key).unwrap().id)
        })
    });
    let mut atoms: Vec<String> = Vec::new();
    if provider_route {
        append_unique_atoms(&mut atoms, &["measure", "compose", "flow", "preserve"]);
    }
    for key in scored.into_iter().map(|(key, _)| key.to_string()) {
        if !atoms.iter().any(|existing| existing == &key) {
            atoms.push(key);
        }
    }
    if atoms.is_empty() {
        atoms = ["scan", "project", "compare", "compose", "measure"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    if provider_route {
        append_unique_atoms(&mut atoms, &["scan", "project"]);
    }
    atoms.truncate(6);
    atoms
}

fn provider_route_required(intent: &str) -> bool {
    let tokens = intent_tokens(intent);
    provider_route_required_from_tokens(&tokens, &classify_intent_without_provider_forcing(&tokens))
}

fn provider_requested_from_tokens(tokens: &[String]) -> bool {
    let provider_terms = [
        "provider", "api", "model", "openai", "llm", "rag", "chatgpt", "deepseek", "mistral",
        "ollama",
    ];
    tokens
        .iter()
        .any(|token| provider_terms.iter().any(|term| token == term))
}

fn provider_route_required_from_tokens(tokens: &[String], atoms: &[String]) -> bool {
    provider_requested_from_tokens(tokens) || provider_signature_atoms(atoms) >= 3
}

fn provider_signature_atoms(atoms: &[String]) -> usize {
    ["measure", "compose", "flow", "preserve"]
        .into_iter()
        .filter(|required| atoms.iter().any(|atom| atom == required))
        .count()
}

fn classify_intent_without_provider_forcing(tokens: &[String]) -> Vec<String> {
    let mut scored: Vec<(&str, i32)> = atoms()
        .iter()
        .map(|atom| {
            let score = atom
                .keywords
                .iter()
                .filter(|keyword| tokens_match_keyword(tokens, keyword))
                .count() as i32;
            (atom.key, score)
        })
        .filter(|(_, score)| *score > 0)
        .collect();
    scored.sort_by(|(a_key, a_score), (b_key, b_score)| {
        b_score.cmp(a_score).then_with(|| {
            atom_by_key(a_key)
                .unwrap()
                .id
                .cmp(&atom_by_key(b_key).unwrap().id)
        })
    });
    scored.into_iter().map(|(key, _)| key.to_string()).collect()
}

fn append_unique_atoms(atoms: &mut Vec<String>, required: &[&str]) {
    for atom in required {
        if !atoms.iter().any(|existing| existing == atom) {
            atoms.push((*atom).to_string());
        }
    }
}

fn intent_tokens(intent: &str) -> Vec<String> {
    intent
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn tokens_match_keyword(tokens: &[String], keyword: &str) -> bool {
    let keyword_tokens = intent_tokens(keyword);
    if keyword_tokens.is_empty() {
        return false;
    }
    tokens
        .windows(keyword_tokens.len())
        .any(|window| window == keyword_tokens.as_slice())
}

fn select_recipe(
    intent: &str,
    atoms: &[String],
    evidence: &[Evidence],
    provider_ready: bool,
    provider_route: bool,
) -> &'static Recipe {
    let mut candidates: Vec<(&Recipe, i32)> = recipes()
        .iter()
        .map(|recipe| {
            let atom_overlap = recipe
                .atoms
                .iter()
                .filter(|atom| atoms.iter().any(|key| key == **atom))
                .count() as i32;
            let evidence_score = evidence
                .iter()
                .filter(|item| item.node_id == recipe.id)
                .map(|item| item.score)
                .max()
                .unwrap_or(0);
            let provider_penalty = if recipe.requires_provider && !provider_ready && !provider_route
            {
                -30
            } else {
                0
            };
            let stack_score = stack_quality(atoms, recipe.atoms);
            let fit_bonus = intent_fit_bonus(intent, recipe, provider_route);
            let complexity = recipe_complexity(recipe);
            let score =
                atom_overlap * 8 + stack_score + evidence_score + fit_bonus + provider_penalty
                    - complexity;
            (recipe, score)
        })
        .collect();
    candidates.sort_by(|(a_recipe, a_score), (b_recipe, b_score)| {
        b_score
            .cmp(a_score)
            .then_with(|| recipe_complexity(a_recipe).cmp(&recipe_complexity(b_recipe)))
            .then_with(|| a_recipe.id.cmp(b_recipe.id))
    });
    candidates[0].0
}

fn intent_fit_bonus(intent: &str, recipe: &Recipe, provider_route: bool) -> i32 {
    let tokens = intent_tokens(intent);
    let renderer_terms = [
        "renderer", "render", "artifact", "native", "pmre", "surface",
    ]
    .iter()
    .any(|term| tokens.iter().any(|token| token == term));
    match recipe.kind {
        "renderer" if renderer_terms && tokens.iter().any(|token| token == "only") => 95,
        "renderer" if renderer_terms => 42,
        "provider" if provider_route => 35,
        "retrieval"
            if ["wiki", "graph", "rag"]
                .iter()
                .any(|term| tokens.iter().any(|token| token == term)) =>
        {
            14
        }
        "fabric"
            if ["spiderweb", "bus", "route", "fabric"]
                .iter()
                .any(|term| tokens.iter().any(|token| token == term)) =>
        {
            14
        }
        "product" if renderer_terms && tokens.iter().any(|token| token == "only") => -40,
        "product"
            if ["app", "build", "production", "dashboard", "usable"]
                .iter()
                .any(|term| tokens.iter().any(|token| token == term)) =>
        {
            16
        }
        _ => 0,
    }
}

fn recipe_complexity(recipe: &Recipe) -> i32 {
    recipe.atoms.len() as i32 + recipe.bonds as i32
}

fn recipe_stack(recipe: &Recipe) -> Vec<String> {
    recipe
        .atoms
        .iter()
        .map(|atom| (*atom).to_string())
        .collect()
}

fn stack_quality(observed: &[String], canonical: &[&str]) -> i32 {
    if canonical.is_empty() {
        return -100;
    }
    let mut score = canonical.len() as i32;
    let mut last_pos: Option<usize> = None;
    for atom in canonical {
        if let Some(pos) = observed.iter().position(|seen| seen == atom) {
            if let Some(last) = last_pos {
                if pos > last {
                    score += 6;
                    if pos == last + 1 {
                        score += 4;
                    }
                } else {
                    score -= 8;
                }
            } else {
                score += 3;
            }
            last_pos = Some(pos);
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use math_atoms_bus::BusLayer;

    fn verified_report(
        runtime: &MathAtomsRuntime,
        label: &str,
        text: &str,
    ) -> (
        ProviderExecutionOutput,
        crate::provider::PersistedProviderOutput,
        std::path::PathBuf,
    ) {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-runtime-work-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = math_atoms_work::WorkPlanStore::new(&root);
        let call = runtime.state().last_provider_call.as_ref().unwrap();
        let mut plan = call.work_plan.clone().unwrap();
        plan.expand_files(vec![math_atoms_work::WorkFile {
            path: "src/lib.rs".to_string(),
            purpose: "provider response".to_string(),
            acceptance: vec!["crate checks, tests, and lints cleanly".to_string()],
        }])
        .unwrap();
        let lease = store.acquire(&plan.id).unwrap();
        let manifest = store.write_plan_manifest(&plan).unwrap();
        for packet in &plan.packets {
            let output = match packet.contract {
                math_atoms_work::PacketContract::Envelope => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"complete\",\"checks\":[\"verified\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_work::PacketContract::FileManifest => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"src/lib.rs\",\"purpose\":\"provider response\",\"acceptance\":[\"crate checks, tests, and lints cleanly\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_work::PacketContract::FileArtifact => format!("```rust\n{text}```"),
            };
            store
                .store_packet(&plan, packet, &output, &call.model)
                .unwrap();
        }
        drop(lease);
        let verifier = math_atoms_verification::CandidateVerifier::new(
            &root,
            math_atoms_verification::VerificationPolicy::strict(120).unwrap(),
        );
        let candidate =
            vec![math_atoms_verification::CandidateFile::new("src/lib.rs", text).unwrap()];
        let attempt = verifier.verify_attempt(&plan.id, 1, &candidate).unwrap();
        assert!(attempt.passed, "{}", attempt.failure);
        let verification = verifier.finalize(&attempt).unwrap();
        let evidence =
            crate::provider::persist_provider_output(text, root.join("provider")).unwrap();
        (
            ProviderExecutionOutput {
                text: text.to_string(),
                work_plan_id: plan.id,
                work_plan_manifest: manifest.to_string_lossy().to_string(),
                packet_ids: plan
                    .packets
                    .iter()
                    .map(|packet| packet.id.clone())
                    .collect(),
                executed_packets: plan.packets.len(),
                resumed_packets: 0,
                candidate_verification: Some(crate::provider::CandidateVerificationReport {
                    manifest_path: verification.manifest_path.to_string_lossy().to_string(),
                    manifest_hash: verification.manifest_hash,
                    bundle_hash: verification.bundle_hash,
                    attempts: verification.attempts,
                    repairs: verification.repairs,
                }),
            },
            evidence,
            root,
        )
    }

    #[test]
    fn run_intent_routes_through_all_spiderweb_layers() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let run = runtime.run_intent("Build provider api wiki graph rag on the spiderweb bus");
        assert_eq!(run.status, RuntimeStatus::ProviderPending);
        assert!(runtime.bus().contains_layer(BusLayer::L0Transport));
        assert!(runtime.bus().contains_layer(BusLayer::L1Message));
        assert!(runtime.bus().contains_layer(BusLayer::L2Flow));
        assert!(runtime.bus().contains_layer(BusLayer::L3Orchestration));
        assert!(run.provider_call.is_some());
    }

    #[test]
    fn missing_provider_fails_closed() {
        let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_pairs(&[]));
        let run = runtime.run_intent("Run the model provider api against graph evidence");
        assert_eq!(run.status, RuntimeStatus::Blocked);
        assert!(run
            .blockers
            .iter()
            .any(|item| item.contains("OPENAI_API_KEY")));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProviderBlocked));
    }

    #[test]
    fn provider_config_apply_clears_stale_proof_state() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Run provider api with wiki graph rag");
        assert_eq!(runtime.state().status, RuntimeStatus::ProviderPending);
        runtime.set_provider(ProviderConfig::from_values(
            "ollama",
            "gpt-oss:120b",
            "https://ollama.com/api/chat",
            "OLLAMA_API_KEY",
        ));
        assert_eq!(runtime.provider().kind.as_str(), "ollama");
        assert_eq!(runtime.state().status, RuntimeStatus::Draft);
        assert!(runtime.state().last_provider_call.is_none());
        assert!(runtime.state().last_route.is_empty());
    }

    #[test]
    fn production_app_build_is_the_product_mission() {
        let m = mission();
        assert!(m.body.contains("requested app"));
        assert!(m.body.contains("canonical atom stacks"));
        assert_eq!(m.readiness_floor, "requested app behavior");
    }

    #[test]
    fn stack_quality_rewards_canonical_order_over_same_atom_bag() {
        let recipe = recipes()
            .iter()
            .find(|recipe| recipe.id == "production-app-runtime")
            .unwrap();
        let canonical = recipe_stack(recipe);
        let mut reversed = canonical.clone();
        reversed.reverse();
        assert!(stack_quality(&canonical, recipe.atoms) > stack_quality(&reversed, recipe.atoms));
    }

    #[test]
    fn renderer_only_intent_prefers_renderer_despite_product_stack_overlap() {
        let atoms = classify_intent("native renderer artifact only");
        let evidence = WikiGraph::seeded().retrieve("native renderer artifact only", &atoms, 8);
        let recipe = select_recipe(
            "native renderer artifact only",
            &atoms,
            &evidence,
            true,
            provider_route_required("native renderer artifact only"),
        );
        assert_eq!(recipe.id, "native-atom-renderer");
    }

    #[test]
    fn provider_detection_uses_tokens_not_substrings() {
        for intent in [
            "Show current storage and drag-and-drop ordering for the business dashboard",
            "Rapidly review the login dialog",
            "Quarterly profit for individual capital accounts",
            "Remodel the local shell layout",
        ] {
            assert!(
                !provider_requested_from_tokens(&intent_tokens(intent)),
                "{intent} should not request a provider"
            );
        }
        assert!(provider_requested_from_tokens(&intent_tokens(
            "Run the provider api model with graph rag"
        )));
    }

    #[test]
    fn atom_classification_uses_tokens_not_embedded_substrings() {
        let tokens = intent_tokens("Fix the login dialog logic for a business review");
        let atoms = classify_intent_without_provider_forcing(&tokens);
        assert!(
            atoms.is_empty(),
            "embedded substrings should not classify atoms: {atoms:?}"
        );
    }

    #[test]
    fn provider_forced_atoms_survive_truncation() {
        let atoms = classify_intent("provider model wiki graph rag from typed input");
        for required in ["measure", "compose", "flow", "preserve"] {
            assert!(
                atoms.iter().any(|atom| atom == required),
                "{required} was dropped from {atoms:?}"
            );
        }
        assert!(atoms.len() <= 6);
    }

    #[test]
    fn provider_intent_selects_provider_recipe_even_without_key() {
        let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_pairs(&[]));
        let run = runtime.run_intent("provider model wiki graph rag from typed input");
        assert_eq!(run.recipe_id, "provider-model-loop");
        assert_eq!(run.status, RuntimeStatus::Blocked);
        assert!(run
            .blockers
            .iter()
            .any(|item| item.contains("OPENAI_API_KEY")));
    }

    #[test]
    fn provider_signature_atoms_fail_closed_without_provider_keyword() {
        let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_pairs(&[]));
        let run = runtime.run_intent(
            "Compose a nested orchestrator that preserve the budget invariant and flow along the fabric while observe telemetry",
        );
        assert_eq!(run.recipe_id, "provider-model-loop");
        assert_eq!(run.status, RuntimeStatus::Blocked);
        assert!(run
            .blockers
            .iter()
            .any(|item| item.contains("OPENAI_API_KEY")));
    }

    #[test]
    fn proof_state_records_selected_recipe_stack() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Build a provider api app with graph evidence and proof");
        let recipe = recipes()
            .iter()
            .find(|recipe| recipe.id == runtime.state().selected_recipe)
            .unwrap();
        assert_eq!(runtime.state().selected_atoms, recipe_stack(recipe));
    }

    #[test]
    fn learned_proof_records_feed_next_retrieval() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.learn_proof_record(&ProofRecord {
            recipe_id: "wiki-graph-rag".to_string(),
            status: "proven".to_string(),
            atoms: vec!["scan".to_string(), "hash".to_string()],
            evidence_count: 4,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: String::new(),
            provider_output_hash: "fnv:0011223344556677".to_string(),
            provider_output_len: 24,
            work_plan_id: String::new(),
            work_plan_manifest: String::new(),
            work_packet_count: 0,
            route_len: 4,
        });
        let run = runtime.run_intent("Use stored proof for wiki graph rag");
        assert!(run
            .evidence
            .iter()
            .any(|item| item.node_id.starts_with("proof:")));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::StoreLearned));
    }

    #[test]
    fn provider_execution_failure_marks_runtime_blocked() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Run provider api with wiki graph rag");
        runtime.schedule_provider_execution().unwrap();
        runtime.mark_provider_blocked("provider returned 401");
        assert_eq!(runtime.state().status, RuntimeStatus::Blocked);
        assert!(runtime
            .state()
            .blockers
            .iter()
            .any(|item| item == "provider returned 401"));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProviderBlocked));
        assert!(runtime
            .bus()
            .route_contains_all_layers(*runtime.state().last_route.last().unwrap()));
    }

    #[test]
    fn malformed_provider_report_blocks_without_completion_evidence() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Run provider api with wiki graph rag");
        let report = ProviderExecutionOutput {
            text: "provider output".to_string(),
            work_plan_id: "work-provider-runtime-test".to_string(),
            work_plan_manifest: "C:/audit/plan-expanded.json".to_string(),
            packet_ids: (1..=13).map(|index| format!("packet-{index}")).collect(),
            executed_packets: 13,
            resumed_packets: 0,
            candidate_verification: None,
        };
        assert!(!runtime.mark_provider_execution_report(
            "C:/audit/output.txt",
            "sha256:abc",
            17,
            &report
        ));
        assert_eq!(runtime.state().status, RuntimeStatus::Blocked);
        assert!(runtime
            .state()
            .blockers
            .iter()
            .any(|blocker| blocker.contains("report verification failed")));
        assert!(!runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::WorkPlanCompleted));
        assert!(!runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProofCaptured));
    }

    #[test]
    fn verified_provider_report_is_pending_real_harness_evidence() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Run provider api with wiki graph rag");
        let pending_route = runtime.state().last_route.clone();
        let task = runtime.schedule_provider_execution().unwrap();
        assert!(!task.route.is_empty());
        assert!(task.route.starts_with(&pending_route));
        assert_eq!(task.call.model, "gpt-5.5");
        let source = "pub fn provider_output() -> &'static str { \"provider output\" }\n";
        let (report, evidence, root) = verified_report(&runtime, "valid-report", source);
        assert!(runtime.mark_provider_execution_report(
            &evidence.path.to_string_lossy(),
            &evidence.hash,
            evidence.len,
            &report
        ));
        assert_eq!(runtime.state().status, RuntimeStatus::VerificationPending);
        assert_eq!(runtime.state().proof_count, 0);
        assert_eq!(
            runtime.state().last_provider_output_artifact,
            evidence.path.to_string_lossy()
        );
        assert_eq!(runtime.state().last_provider_output_hash, evidence.hash);
        assert_eq!(runtime.state().last_provider_output_len, source.len());
        assert_eq!(runtime.state().last_work_plan_id, report.work_plan_id);
        assert_eq!(
            runtime.state().last_work_plan_manifest,
            report.work_plan_manifest
        );
        assert_eq!(runtime.state().last_work_packet_count, 19);
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProviderExecutionScheduled));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProviderExecuted));
        assert_eq!(
            runtime
                .bus()
                .envelopes()
                .iter()
                .filter(|env| env.kind == BusMessageKind::WorkPacketExecuted)
                .count(),
            report.packet_ids.len()
        );
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::WorkPlanCompleted));
        assert!(!runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProofCaptured));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProofPending));
        assert!(runtime
            .bus()
            .route_contains_all_layers(*runtime.state().last_route.last().unwrap()));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn provider_required_routes_do_not_claim_proven_before_execution() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let run = runtime.run_intent("Run provider api with wiki graph rag");
        assert_eq!(run.status, RuntimeStatus::ProviderPending);
        assert_eq!(runtime.state().proof_count, 0);
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProofPending));
        assert!(!runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProofCaptured));
    }

    #[test]
    fn store_failure_marks_runtime_blocked() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        runtime.run_intent("Build provider api wiki graph rag");
        runtime.mark_store_blocked("persistent proof store write failed");
        assert_eq!(runtime.state().status, RuntimeStatus::Blocked);
        assert!(runtime
            .state()
            .blockers
            .iter()
            .any(|item| item == "persistent proof store write failed"));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::StoreBlocked));
    }

    #[test]
    fn corrupt_proof_store_blocks_runtime_startup() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-corrupt-startup-store-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "corrupt proof record\n").unwrap();
        let runtime = MathAtomsRuntime::with_proof_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            ProofStore::new(&path),
        );
        std::fs::remove_file(&path).ok();
        assert_eq!(runtime.state().status, RuntimeStatus::Blocked);
        assert!(runtime
            .state()
            .blockers
            .iter()
            .any(|item| item.contains("Proof store read failed")));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::StoreBlocked));
        assert!(runtime
            .bus()
            .route_contains_all_layers(*runtime.state().last_route.last().unwrap()));
    }
}
