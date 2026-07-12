use crate::contracts::step_output_contract;
use crate::model::{
    provider_identity, safe_relative, ExecutedTurn, PreparedTurn, ProviderResultRoute,
    RuntimeError, RuntimePaths, SessionManifest,
};
use crate::session::{json_escape, SessionStore};
use crate::turns::{NewTurnRecord, TurnStore};
use atom_vibe_build_gates::GateInput;
use atom_vibe_build_planner::{
    BuildCoordinator, BuildLedger, BuildPlannerDecision, BuildRunStatus,
};
use atom_vibe_context::{CoderContextAssembler, CoderContextRequest};
use atom_vibe_mode::provider_system_prompt;
use atom_vibe_provider::{
    ProviderAdapter, ProviderCredentialSource, ProviderHttp, ProviderRequest,
};
use atom_vibe_scratchpad::{
    ScratchpadEntryKind, ScratchpadScope, ScratchpadStore, MAX_ENTRY_BYTES,
};
use math_atoms_bus::{BusMessageKind, SpiderwebBus};
use math_atoms_core::ProviderConfig;
use math_atoms_hash::sha256_tagged;
use math_atoms_provider_transport::persist_provider_output;
use std::fs;
use std::path::PathBuf;

pub struct AtomVibeRuntime {
    paths: RuntimePaths,
    sessions: SessionStore,
    turns: TurnStore,
    coordinator: BuildCoordinator,
    context: CoderContextAssembler,
    provider: ProviderAdapter,
    provider_bus: SpiderwebBus,
    provider_routes: Vec<ProviderResultRoute>,
}

impl AtomVibeRuntime {
    pub fn open(
        root: impl Into<PathBuf>,
        provider_config: ProviderConfig,
    ) -> Result<Self, RuntimeError> {
        let requested = root.into();
        fs::create_dir_all(&requested)
            .map_err(|error| RuntimeError::InvalidConfiguration(error.to_string()))?;
        let root = requested
            .canonicalize()
            .map_err(|error| RuntimeError::InvalidConfiguration(error.to_string()))?;
        let paths = RuntimePaths::new(root);
        for path in [
            &paths.planners,
            &paths.sessions,
            &paths.scratchpads,
            &paths.outputs,
            &paths.turns,
        ] {
            fs::create_dir_all(path)
                .map_err(|error| RuntimeError::InvalidConfiguration(error.to_string()))?;
        }
        let sessions = SessionStore::open(&paths.sessions)?;
        let turns = TurnStore::open(&paths.turns, &paths.root)?;
        let coordinator = BuildCoordinator::open(&paths.planners)
            .map_err(|error| RuntimeError::Planner(error.to_string()))?;
        let context = CoderContextAssembler::from_default_wiki()
            .map_err(|error| RuntimeError::Context(error.to_string()))?;
        let provider = ProviderAdapter::new(provider_config)?;
        Ok(Self {
            paths,
            sessions,
            turns,
            coordinator,
            context,
            provider,
            provider_bus: SpiderwebBus::new(),
            provider_routes: Vec::new(),
        })
    }

    pub fn paths(&self) -> &RuntimePaths {
        &self.paths
    }

    pub fn provider(&self) -> &ProviderAdapter {
        &self.provider
    }

    pub fn set_provider(&mut self, config: ProviderConfig) -> Result<(), RuntimeError> {
        self.provider = ProviderAdapter::new(config)?;
        Ok(())
    }

    pub fn start_build(
        &mut self,
        project_id: &str,
        operator_request: &str,
    ) -> Result<SessionManifest, RuntimeError> {
        if operator_request.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "natural-language request is empty".to_string(),
            ));
        }
        let build_id = self
            .coordinator
            .start_build(project_id, true, 6)
            .map_err(|error| RuntimeError::Planner(error.to_string()))?;
        let manifest = self.sessions.create(
            &build_id,
            project_id,
            operator_request,
            self.provider.config(),
        )?;
        self.scratchpad(&build_id)?
            .append(
                None,
                ScratchpadEntryKind::Observation,
                operator_request,
                &["operator:natural-language-request".to_string()],
            )
            .map_err(|error| RuntimeError::Scratchpad(error.to_string()))?;
        Ok(manifest)
    }

    pub fn session(&self, build_id: &str) -> Result<SessionManifest, RuntimeError> {
        self.sessions.load(build_id)
    }

    pub fn prepare_turn(&mut self, build_id: &str) -> Result<PreparedTurn, RuntimeError> {
        let session = self.sessions.load(build_id)?;
        let ledger = self.active_ledger(build_id)?.clone();
        let scratchpad = self.scratchpad(build_id)?;
        let mut context_request =
            CoderContextRequest::new(build_id, &session.operator_request, ledger.current_step);
        context_request.failure_context = latest_failure(&ledger);
        let context = self
            .context
            .prepare(&context_request, &scratchpad)
            .map_err(|error| RuntimeError::Context(error.to_string()))?;
        let mode = provider_system_prompt(ledger.current_step)
            .map_err(|error| RuntimeError::Mode(error.to_string()))?;
        let system_instructions = format!(
            "{mode}\n\n# Context controller\n\n{}\n\n# Required output contract\n\n{}",
            context.system_instructions,
            step_output_contract(ledger.current_step)
        );
        let data = turn_data(&context.data, &ledger);
        let request_id = format!(
            "{}:{}:{}:{}",
            build_id,
            ledger.current_step.as_str(),
            ledger.revision,
            context.scratchpad.entry_count.saturating_add(1)
        );
        let request = ProviderRequest::new(request_id, system_instructions, data);
        request.validate()?;
        Ok(PreparedTurn {
            build_id: build_id.to_string(),
            step: ledger.current_step,
            planner_revision: ledger.revision,
            provider_identity_hash: self.provider_identity_hash(),
            context,
            request,
        })
    }

    pub fn execute_turn(&mut self, prepared: &PreparedTurn) -> Result<ExecutedTurn, RuntimeError> {
        self.execute_turn_with(
            prepared,
            &atom_vibe_provider::CurlProviderHttp,
            &atom_vibe_provider::EnvironmentCredentials,
        )
    }

    pub fn execute_turn_with(
        &mut self,
        prepared: &PreparedTurn,
        http: &dyn ProviderHttp,
        credentials: &dyn ProviderCredentialSource,
    ) -> Result<ExecutedTurn, RuntimeError> {
        self.validate_prepared(prepared)?;
        let evidence_ids = prepared
            .context
            .evidence
            .iter()
            .map(|item| item.node_id.clone())
            .collect::<Vec<_>>();
        let receipt = match self
            .provider
            .execute_with(&prepared.request, http, credentials)
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let route = self.emit_provider_route(
                    true,
                    &prepared.build_id,
                    &error.to_string(),
                    &evidence_ids,
                );
                let failure = format!(
                    "Provider turn {} blocked after Spiderweb route {:?}: {}",
                    prepared.request.request_id, route.route, error
                );
                self.scratchpad(&prepared.build_id)?
                    .append(
                        Some(prepared.step),
                        ScratchpadEntryKind::GateFailure,
                        &failure,
                        &evidence_ids,
                    )
                    .map_err(|scratchpad| {
                        RuntimeError::Scratchpad(format!(
                            "{scratchpad}; original provider failure: {error}"
                        ))
                    })?;
                return Err(RuntimeError::Provider(error));
            }
        };
        let persisted =
            persist_provider_output(&receipt.text, self.paths.outputs.join(&prepared.build_id))
                .map_err(|error| RuntimeError::TurnStore(error.to_string()))?;
        if persisted.hash != receipt.output_hash {
            return Err(RuntimeError::TurnStore(
                "persisted provider output hash differs from receipt".to_string(),
            ));
        }
        let relative_path = persisted
            .path
            .strip_prefix(&self.paths.root)
            .ok()
            .and_then(safe_relative)
            .ok_or_else(|| {
                RuntimeError::TurnStore(
                    "provider output was not stored below the runtime root".to_string(),
                )
            })?;
        let result_route = self.emit_provider_route(
            false,
            &prepared.build_id,
            &receipt.output_hash,
            &evidence_ids,
        );
        let scratchpad_text =
            scratchpad_output(&receipt.text, &relative_path, &receipt.output_hash);
        let mut source_ids = evidence_ids.clone();
        source_ids.push(receipt.output_hash.clone());
        let scratchpad_entry = self
            .scratchpad(&prepared.build_id)?
            .append(
                Some(prepared.step),
                ScratchpadEntryKind::PacketOutput,
                &scratchpad_text,
                &source_ids,
            )
            .map_err(|error| RuntimeError::Scratchpad(error.to_string()))?;
        let record = self.turns.append(NewTurnRecord {
            build_id: &prepared.build_id,
            step: prepared.step,
            planner_revision: prepared.planner_revision,
            receipt: &receipt,
            output_artifact: &relative_path,
            evidence_ids: &evidence_ids,
            context_route: &prepared.context.route.route,
            result_route: &result_route.route,
            scratchpad_entry_hash: &scratchpad_entry.entry_hash,
        })?;
        Ok(ExecutedTurn {
            receipt,
            output_artifact: persisted.path,
            result_route,
            record,
        })
    }

    pub fn evaluate_gate(
        &mut self,
        build_id: &str,
        input: &GateInput,
    ) -> Result<BuildPlannerDecision, RuntimeError> {
        self.sessions.load(build_id)?;
        let decision = self
            .coordinator
            .evaluate(build_id, input)
            .map_err(|error| RuntimeError::Planner(error.to_string()))?;
        let ledger = self
            .coordinator
            .get(build_id)
            .ok_or_else(|| RuntimeError::Planner("evaluated build disappeared".to_string()))?
            .ledger()
            .clone();
        let (kind, content, sources) = decision_scratchpad(&ledger, &decision);
        let scratchpad = self.scratchpad(build_id)?;
        scratchpad
            .append(Some(input.step), kind, &content, &sources)
            .map_err(|error| RuntimeError::Scratchpad(error.to_string()))?;
        if ledger.status == BuildRunStatus::Complete {
            scratchpad
                .seal()
                .map_err(|error| RuntimeError::Scratchpad(error.to_string()))?;
        }
        Ok(decision)
    }

    pub fn ledgers(&self) -> Vec<BuildLedger> {
        self.coordinator.ledgers()
    }

    pub fn turn_records(&self, build_id: &str) -> Result<Vec<crate::TurnRecord>, RuntimeError> {
        self.turns.load(build_id)
    }

    pub fn provider_bus(&self) -> &SpiderwebBus {
        &self.provider_bus
    }

    pub fn provider_routes(&self) -> &[ProviderResultRoute] {
        &self.provider_routes
    }

    fn active_ledger(&self, build_id: &str) -> Result<&BuildLedger, RuntimeError> {
        let ledger = self
            .coordinator
            .get(build_id)
            .ok_or_else(|| RuntimeError::Planner(format!("build {build_id} was not found")))?
            .ledger();
        if ledger.status != BuildRunStatus::Active {
            return Err(RuntimeError::BuildNotActive(build_id.to_string()));
        }
        Ok(ledger)
    }

    fn validate_prepared(&self, prepared: &PreparedTurn) -> Result<(), RuntimeError> {
        let ledger = self.active_ledger(&prepared.build_id)?;
        if ledger.current_step != prepared.step
            || ledger.revision != prepared.planner_revision
            || self.provider_identity_hash() != prepared.provider_identity_hash
        {
            return Err(RuntimeError::StalePreparedTurn);
        }
        Ok(())
    }

    fn scratchpad(&self, build_id: &str) -> Result<ScratchpadStore, RuntimeError> {
        let scope = ScratchpadScope::new(build_id, &provider_identity(self.provider.config()))
            .map_err(|error| RuntimeError::Scratchpad(error.to_string()))?;
        ScratchpadStore::open(&self.paths.scratchpads, scope)
            .map_err(|error| RuntimeError::Scratchpad(error.to_string()))
    }

    fn provider_identity_hash(&self) -> String {
        sha256_tagged(provider_identity(self.provider.config()).as_bytes())
    }

    fn emit_provider_route(
        &mut self,
        blocked: bool,
        build_id: &str,
        body: &str,
        evidence_ids: &[String],
    ) -> ProviderResultRoute {
        let kind = if blocked {
            BusMessageKind::ProviderBlocked
        } else {
            BusMessageKind::ProviderExecuted
        };
        let ingress =
            self.provider_bus
                .l0_transport(kind, "provider-model", "provider-off-ramp", body);
        let message = self.provider_bus.l1_message(
            ingress,
            kind,
            "provider-off-ramp",
            "provider-receipt",
            build_id,
        );
        let flow = self.provider_bus.l2_flow(
            message,
            kind,
            "provider-receipt",
            "coder-turn-store",
            body,
            evidence_ids,
        );
        let terminal = self.provider_bus.l3_orchestrate(
            flow,
            kind,
            "coder-turn-store",
            "atom-vibe-runtime",
            build_id,
            evidence_ids,
        );
        let route = ProviderResultRoute {
            terminal,
            route: self
                .provider_bus
                .route_for(terminal)
                .iter()
                .map(|envelope| envelope.id)
                .collect(),
            blocked,
        };
        self.provider_routes.push(route.clone());
        route
    }
}

fn turn_data(context_json: &str, ledger: &BuildLedger) -> String {
    let failure = latest_failure(ledger);
    format!(
        "{{\"coder_context\":{},\"planner_state\":{{\"build_id\":\"{}\",\"project_id\":\"{}\",\"revision\":{},\"status\":\"{}\",\"current_step\":\"{}\",\"current_skill\":\"{}\",\"retry_attempts\":{},\"deferred_debt_count\":{},\"couple_marker_count\":{},\"latest_verified_failure\":\"{}\"}}}}",
        context_json,
        json_escape(&ledger.build_id),
        json_escape(&ledger.project_id),
        ledger.revision,
        ledger.status.as_str(),
        ledger.current_step.as_str(),
        ledger.current_skill(),
        ledger.retry_attempts(ledger.current_step),
        ledger.deferred_debt.len(),
        ledger.couple_markers.len(),
        json_escape(&failure)
    )
}

fn latest_failure(ledger: &BuildLedger) -> String {
    ledger
        .retry_ledger
        .iter()
        .rev()
        .find(|record| record.step == ledger.current_step)
        .map(|record| record.summary.clone())
        .unwrap_or_default()
}

fn scratchpad_output(text: &str, artifact: &str, hash: &str) -> String {
    let header = format!("Provider output artifact: {artifact}\nOutput hash: {hash}\n");
    let available = MAX_ENTRY_BYTES.saturating_sub(header.len() + 64);
    let mut end = text.len().min(available);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == text.len() {
        format!("{header}Output:\n{text}")
    } else {
        format!(
            "{header}Output projection (full output remains in artifact):\n{}\n[projection truncated]",
            &text[..end]
        )
    }
}

fn decision_scratchpad(
    ledger: &BuildLedger,
    decision: &BuildPlannerDecision,
) -> (ScratchpadEntryKind, String, Vec<String>) {
    let sources = match decision {
        BuildPlannerDecision::Advance(_) => ledger
            .step_outputs
            .last()
            .map(|output| output.artifacts.as_slice()),
        BuildPlannerDecision::AutonomousRetry(_) | BuildPlannerDecision::HardHalt(_) => ledger
            .retry_ledger
            .last()
            .map(|record| record.artifacts.as_slice()),
        BuildPlannerDecision::Deferred(_) => None,
    }
    .unwrap_or_default()
    .iter()
    .map(|artifact| format!("artifact:{}", artifact.path))
    .collect::<Vec<_>>();
    let kind = match decision {
        BuildPlannerDecision::Advance(_) => ScratchpadEntryKind::Checkpoint,
        BuildPlannerDecision::AutonomousRetry(_) | BuildPlannerDecision::HardHalt(_) => {
            ScratchpadEntryKind::GateFailure
        }
        BuildPlannerDecision::Deferred(_) => ScratchpadEntryKind::Decision,
    };
    let detail = ledger
        .retry_ledger
        .last()
        .filter(|record| record.step == ledger.current_step)
        .map(|record| record.summary.as_str())
        .unwrap_or("");
    (
        kind,
        format!(
            "Deterministic gate decision: {}. Planner revision: {}. Current step: {}. {}",
            decision.label(),
            ledger.revision,
            ledger.current_step.as_str(),
            detail
        ),
        sources,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_vibe_build_gates::RequirementsRecord;
    use atom_vibe_build_protocol::{BuildArtifactRef, BuildStep};
    use atom_vibe_provider::{ProviderAdapterError, ProviderCredentialSource, ProviderHttp};
    use math_atoms_hash::sha256_hex;
    use math_atoms_provider_transport::{ProviderHttpRequest, ProviderTransportError};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FixedCredential(String);

    impl ProviderCredentialSource for FixedCredential {
        fn load(&self, _name: &str) -> Result<String, ProviderAdapterError> {
            Ok(self.0.clone())
        }
    }

    struct FixedHttp {
        called: AtomicBool,
    }

    impl ProviderHttp for FixedHttp {
        fn post_json(
            &self,
            _request: ProviderHttpRequest<'_>,
        ) -> Result<String, ProviderTransportError> {
            self.called.store(true, Ordering::SeqCst);
            Ok(r#"{"choices":[{"message":{"reasoning_content":"checked every field","content":"{\"schema_version\":1,\"build_id\":\"test\",\"step\":\"intake\",\"summary\":\"complete\",\"payload\":{}}"}}],"usage":{"prompt_tokens":12,"completion_tokens":8,"completion_tokens_details":{"reasoning_tokens":4}}}"#.to_string())
        }
    }

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-runtime-{label}-{}-{}",
            std::process::id(),
            atom_vibe_build_protocol::unix_time_ms()
        ))
    }

    fn config(model: &str, key: &str) -> ProviderConfig {
        ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_FORMAT", "chat"),
            (
                "MATH_ATOMS_PROVIDER_URL",
                "http://127.0.0.1:1234/v1/chat/completions",
            ),
            ("MATH_ATOMS_PROVIDER_MODEL", model),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", "ATOM_VIBE_TEST_KEY"),
            ("MATH_ATOMS_PROVIDER_THINKING_LEVEL", "low"),
            ("ATOM_VIBE_TEST_KEY", key),
        ])
    }

    fn artifact(root: &Path, name: &str, text: &str) -> BuildArtifactRef {
        let relative = format!("evidence/{name}.txt");
        let path = root.join(&relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        BuildArtifactRef::new(relative, sha256_hex(text.as_bytes()), name).unwrap()
    }

    fn valid_intake(root: &Path) -> GateInput {
        let mut input = GateInput::new(BuildStep::Intake, root);
        input.requirements = Some(RequirementsRecord {
            purpose: "track inventory".to_string(),
            user_behaviors: vec!["add inventory".to_string()],
            ui_decision: "native PMRE".to_string(),
            persistence_decision: "durable local state".to_string(),
            external_boundaries: vec!["none".to_string()],
            execution_siting: "local".to_string(),
            out_of_scope: vec!["cloud sync".to_string()],
            definition_of_done: vec!["inventory survives restart".to_string()],
            artifact: artifact(root, "requirements", "complete requirements"),
        });
        input
    }

    #[test]
    fn turn_composes_graph_scratchpad_provider_bus_and_restart_evidence() {
        let root = root("compose");
        let key = "runtime-unit-test-key";
        let mut runtime = AtomVibeRuntime::open(&root, config("qwen3.5-9b-q8", key)).unwrap();
        let session = runtime
            .start_build("inventory", "Build a native inventory dashboard")
            .unwrap();
        let prepared = runtime.prepare_turn(&session.build_id).unwrap();
        assert!(prepared
            .request
            .system_instructions
            .contains("Qwen3.5 9B Q8"));
        assert!(prepared
            .context
            .evidence
            .iter()
            .any(|item| item.node_id.starts_with("wiki:thinking-model-requirements")));
        assert_eq!(prepared.context.route.route.len(), 4);
        let http = FixedHttp {
            called: AtomicBool::new(false),
        };
        let executed = runtime
            .execute_turn_with(&prepared, &http, &FixedCredential(key.to_string()))
            .unwrap();
        assert!(http.called.load(Ordering::SeqCst));
        assert!(executed.output_artifact.is_file());
        assert_eq!(executed.result_route.route.len(), 4);
        assert!(runtime
            .provider_bus()
            .route_contains_all_layers(executed.result_route.terminal));
        drop(runtime);

        let mut restarted = AtomVibeRuntime::open(&root, config("qwen3.5-9b-q8", key)).unwrap();
        assert_eq!(restarted.turn_records(&session.build_id).unwrap().len(), 1);
        let resumed = restarted.prepare_turn(&session.build_id).unwrap();
        assert!(resumed
            .context
            .scratchpad
            .text
            .contains("Provider output artifact"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_turn_and_provider_scope_switch_fail_closed() {
        let root = root("stale");
        let key = "runtime-unit-test-key";
        let mut runtime = AtomVibeRuntime::open(&root, config("qwen3.5-9b-q8", key)).unwrap();
        let session = runtime
            .start_build("notes", "Build a native notes app")
            .unwrap();
        let prepared = runtime.prepare_turn(&session.build_id).unwrap();
        let first_scope = prepared.context.scratchpad.model_scope_hash.clone();
        runtime
            .set_provider(config("stronger-thinking-model", key))
            .unwrap();
        let switched = runtime.prepare_turn(&session.build_id).unwrap();
        assert_ne!(first_scope, switched.context.scratchpad.model_scope_hash);
        assert_eq!(
            runtime
                .execute_turn_with(
                    &prepared,
                    &FixedHttp {
                        called: AtomicBool::new(false)
                    },
                    &FixedCredential(key.to_string())
                )
                .unwrap_err(),
            RuntimeError::StalePreparedTurn
        );

        runtime.set_provider(config("qwen3.5-9b-q8", key)).unwrap();
        let current = runtime.prepare_turn(&session.build_id).unwrap();
        runtime
            .evaluate_gate(&session.build_id, &valid_intake(&root))
            .unwrap();
        let http = FixedHttp {
            called: AtomicBool::new(false),
        };
        assert_eq!(
            runtime
                .execute_turn_with(&current, &http, &FixedCredential(key.to_string()))
                .unwrap_err(),
            RuntimeError::StalePreparedTurn
        );
        assert!(!http.called.load(Ordering::SeqCst));
        fs::remove_dir_all(root).unwrap();
    }
}
