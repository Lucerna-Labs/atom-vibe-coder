use math_atoms_core::{
    default_provider_output_dir, effective_records, persist_provider_output, LearningOutcome,
    LearningRecord, LearningRecordInput, LearningStore, LearningSummary, MathAtomsRuntime,
    PreparedProviderCall, ProofRecord, ProofStore, ProviderConfig, ProviderConfigInput,
    ProviderError, ProviderExecutionOutput, RuntimeStatus, WikiGraph, DEFAULT_GRAPH_MEMORY_LIMIT,
};
use pmre_orchestrator::{
    UiState, DESIGN_ANIMATION_SLIDER, DESIGN_GLASS_SLIDER, DESIGN_HUE_SLIDER, DESIGN_LIGHT_SLIDER,
    DESIGN_RADIUS_SLIDER, DESIGN_SAT_SLIDER, DESIGN_TEXT_SLIDER,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub const INTENT_INPUT: u32 = 1;
pub const RUN_LOOP: u32 = 2;
pub const EXEC_PROVIDER: u32 = 3;
pub const CAPTURE_PROOF: u32 = 4;
pub const MARK_DRIFT: u32 = 5;
pub const EVIDENCE_SCROLL: u32 = 6;
pub const BUS_SCROLL: u32 = 7;
pub const PROVIDER_KIND_INPUT: u32 = 8;
pub const PROVIDER_MODEL_INPUT: u32 = 9;
pub const PROVIDER_URL_INPUT: u32 = 10;
pub const PROVIDER_KEY_ENV_INPUT: u32 = 11;
pub const APPLY_PROVIDER: u32 = 12;
pub const PROVIDER_FORMAT_INPUT: u32 = 14;
pub const PROVIDER_AUTH_HEADER_INPUT: u32 = 15;
pub const PROVIDER_AUTH_SCHEME_INPUT: u32 = 16;
pub const PROVIDER_BODY_TEMPLATE_INPUT: u32 = 17;
pub const PROVIDER_RESPONSE_KEY_INPUT: u32 = 18;
pub const APP_SCROLL: u32 = 19;
pub const SETTINGS_SCROLL: u32 = 20;
pub const WORKSPACE_TAB: u32 = 21;
pub const SETTINGS_TAB: u32 = 22;
pub const PROVIDER_CONNECTIONS_TAB: u32 = 23;
pub const RUNTIME_SETTINGS_TAB: u32 = 24;
pub const ARTIFACT_SCROLL: u32 = 25;
pub const DESIGN_UPLOAD_TAB: u32 = 26;
pub const DESIGN_HTML_PATH_INPUT: u32 = 27;
pub const DESIGN_CSS_PATH_INPUT: u32 = 28;
pub const BUILD_DESIGN_UPLOAD: u32 = 29;
pub const WIKI_TAB: u32 = 30;
pub const MCP_TAB: u32 = 31;
pub const SKILLS_TAB: u32 = 32;
pub const HOOKS_TAB: u32 = 33;

pub fn default_intent() -> &'static str {
    "Build a native Atom Vibe Coder app on the Spiderweb Bus with provider API, wiki graph RAG, proof capture, and side artifact preview."
}

#[derive(Clone, Debug)]
pub struct NativeApp {
    pub runtime: MathAtomsRuntime,
    pub store: Option<ProofStore>,
    pub learning_store: Option<LearningStore>,
    pub learning_records: Vec<LearningRecord>,
    learning_summary: LearningSummary,
    learning_attempts: HashMap<(String, String), u32>,
    pub last_run_summary: String,
    pub last_provider_output: String,
    pub provider_running: bool,
    pub design_build_running: bool,
    pub last_design_output: String,
    pub active_main_tab: MainTab,
    pub active_settings_tab: SettingsTab,
    pub side_artifacts: Vec<SideArtifact>,
    last_intent: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideArtifact {
    pub name: String,
    pub status: String,
    pub output: String,
    pub source_path: String,
    pub exe_path: String,
    pub artifact_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainTab {
    Workspace,
    Provider,
    Wiki,
    Mcp,
    Skills,
    Hooks,
    Settings,
    DesignUpload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsTab {
    ProviderConnections,
    DesignUpload,
    Runtime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoreOutcome {
    Memory,
    Written,
    Blocked(String),
}

impl StoreOutcome {
    fn message(&self) -> String {
        match self {
            Self::Memory => "store:memory".to_string(),
            Self::Written => "store:written".to_string(),
            Self::Blocked(reason) => format!("store:blocked {reason}"),
        }
    }
}

impl NativeApp {
    pub fn from_process_env() -> Self {
        Self::new_with_stores(
            ProviderConfig::from_process_env(),
            Some(ProofStore::new(ProofStore::default_path())),
            Some(LearningStore::new(LearningStore::default_path())),
        )
    }

    #[cfg(test)]
    pub fn new(provider: ProviderConfig) -> Self {
        Self::new_with_store(provider, None)
    }

    #[cfg(test)]
    pub fn new_with_store(provider: ProviderConfig, store: Option<ProofStore>) -> Self {
        let learning_store = store
            .as_ref()
            .map(|proof_store| LearningStore::beside(proof_store.path()));
        Self::new_with_stores(provider, store, learning_store)
    }

    pub fn new_with_stores(
        provider: ProviderConfig,
        store: Option<ProofStore>,
        learning_store: Option<LearningStore>,
    ) -> Self {
        let last_provider_output = initial_provider_output(&provider);
        let archived_learning = learning_store
            .as_ref()
            .and_then(|ledger| ledger.read_records().ok())
            .unwrap_or_default();
        let learning_summary = LearningSummary::from_records(&archived_learning);
        let mut learning_attempts = HashMap::new();
        for record in &archived_learning {
            let key = (record.intent.clone(), record.gate.clone());
            learning_attempts
                .entry(key)
                .and_modify(|attempt: &mut u32| *attempt = (*attempt).max(record.attempt))
                .or_insert(record.attempt);
        }
        let learning_records = effective_records(&archived_learning, DEFAULT_GRAPH_MEMORY_LIMIT);
        let runtime = match (&store, &learning_store) {
            (Some(proofs), Some(learning)) => {
                MathAtomsRuntime::with_stores(provider, proofs.clone(), learning.clone())
            }
            (Some(proofs), None) => MathAtomsRuntime::with_proof_store(provider, proofs.clone()),
            _ => MathAtomsRuntime::with_graph(provider, WikiGraph::from_default_dirs()),
        };
        Self {
            runtime,
            store,
            learning_store,
            learning_records,
            learning_summary,
            learning_attempts,
            last_run_summary: "No proof run yet.".to_string(),
            last_provider_output,
            provider_running: false,
            design_build_running: false,
            last_design_output: "Design upload has not been built.".to_string(),
            active_main_tab: MainTab::Workspace,
            active_settings_tab: SettingsTab::ProviderConnections,
            side_artifacts: load_side_artifacts(),
            last_intent: String::new(),
        }
    }

    pub fn seed_input(&self, ui: &mut UiState) {
        ui.inputs
            .entry(INTENT_INPUT)
            .or_insert_with(|| default_intent().to_string());
        seed_provider_inputs(self.runtime.provider(), ui);
        seed_design_inputs(ui);
        seed_lucerna_design_defaults(ui);
        ui.focused = Some(INTENT_INPUT);
    }

    pub fn run_current_intent(&mut self, ui: &UiState) {
        let intent = current_intent(ui);
        self.last_intent = intent.clone();
        let proof = self.runtime.run_intent(&intent);
        let store_result = self.append_proof_record(true);
        self.last_run_summary = if self.status() == RuntimeStatus::Proven {
            format!(
                "{} proven with {} evidence nodes through {} route envelopes. {}",
                proof.recipe_id,
                proof.evidence.len(),
                self.runtime.state().last_route.len(),
                store_result.message()
            )
        } else if self.status() == RuntimeStatus::ProviderPending {
            format!(
                "{} selected with {} evidence nodes through {} route envelopes. Provider execution required before proof can pass. {}",
                proof.recipe_id,
                proof.evidence.len(),
                self.runtime.state().last_route.len(),
                store_result.message()
            )
        } else {
            format!(
                "Blocked: {} {}",
                self.runtime.state().blockers.join("; "),
                store_result.message()
            )
        };
    }

    pub fn mark_drift(&mut self) {
        self.runtime
            .mark_drift("Operator flagged drift from the native PMRE shell.");
        self.last_run_summary =
            "Drift flagged; next proof run must re-establish evidence.".to_string();
    }

    pub fn show_workspace(&mut self) {
        self.active_main_tab = MainTab::Workspace;
    }

    pub fn show_settings(&mut self) {
        self.active_main_tab = MainTab::Settings;
        self.active_settings_tab = SettingsTab::Runtime;
    }

    pub fn show_provider_connections(&mut self) {
        self.active_main_tab = MainTab::Provider;
        self.active_settings_tab = SettingsTab::ProviderConnections;
    }

    pub fn show_runtime_settings(&mut self) {
        self.active_main_tab = MainTab::Settings;
        self.active_settings_tab = SettingsTab::Runtime;
    }

    pub fn show_design_upload(&mut self) {
        self.active_main_tab = MainTab::DesignUpload;
        self.active_settings_tab = SettingsTab::DesignUpload;
    }

    pub fn show_wiki(&mut self) {
        self.active_main_tab = MainTab::Wiki;
    }

    pub fn show_mcp(&mut self) {
        self.active_main_tab = MainTab::Mcp;
    }

    pub fn show_skills(&mut self) {
        self.active_main_tab = MainTab::Skills;
    }

    pub fn show_hooks(&mut self) {
        self.active_main_tab = MainTab::Hooks;
    }

    pub fn apply_provider_config(&mut self, ui: &UiState) {
        let config = ProviderConfig::from_values_full(ProviderConfigInput {
            kind_raw: ui.input_text(PROVIDER_KIND_INPUT),
            format_raw: ui.input_text(PROVIDER_FORMAT_INPUT),
            model: ui.input_text(PROVIDER_MODEL_INPUT),
            endpoint: ui.input_text(PROVIDER_URL_INPUT),
            api_key_env: ui.input_text(PROVIDER_KEY_ENV_INPUT),
            auth_header: ui.input_text(PROVIDER_AUTH_HEADER_INPUT),
            auth_scheme: ui.input_text(PROVIDER_AUTH_SCHEME_INPUT),
            body_template: ui.input_text(PROVIDER_BODY_TEMPLATE_INPUT),
            response_key: ui.input_text(PROVIDER_RESPONSE_KEY_INPUT),
        });
        let ready = config.is_ready();
        let summary = format!(
            "Provider config applied: {} {} {} via {} ({})",
            config.kind.as_str(),
            config.wire_format.as_str(),
            config.model,
            config.endpoint,
            if ready { "key present" } else { "key missing" }
        );
        self.runtime.set_provider(config);
        self.last_provider_output = if ready {
            "Provider has not been executed.".to_string()
        } else {
            format!(
                "Provider blocked: Missing credential in {}",
                self.runtime.provider().api_key_env
            )
        };
        self.provider_running = false;
        self.last_run_summary = summary;
    }

    pub fn capture_current_proof(&mut self) {
        if self.runtime.state().last_route.is_empty() {
            self.last_run_summary =
                "Capture blocked: run a proof route before storing evidence.".to_string();
            return;
        }
        if self.status() == RuntimeStatus::ProviderPending {
            self.last_run_summary =
                "Capture blocked: provider execution must complete before proof capture."
                    .to_string();
            return;
        }
        let store_result = self.append_proof_record(false);
        self.last_run_summary = if matches!(store_result, StoreOutcome::Blocked(_)) {
            format!("Capture blocked: {}", store_result.message())
        } else {
            format!("Captured current proof route. {}", store_result.message())
        };
    }

    pub fn begin_provider_execution(
        &mut self,
    ) -> Option<Receiver<Result<ProviderExecutionOutput, ProviderError>>> {
        if self.provider_running {
            self.last_provider_output = "Provider request already running.".to_string();
            return None;
        }
        let Some(task) = self.runtime.schedule_provider_execution() else {
            let reason = self
                .runtime
                .state()
                .blockers
                .last()
                .cloned()
                .unwrap_or_else(|| "Provider execution was not scheduled.".to_string());
            self.last_provider_output = format!("Provider blocked: {reason}");
            return None;
        };
        let (tx, rx) = mpsc::channel();
        self.provider_running = true;
        self.last_provider_output = format!(
            "Provider request running through {} Spiderweb route envelopes.",
            task.route.len()
        );
        thread::spawn(move || {
            let _ = tx.send(execute_call(task.call));
        });
        Some(rx)
    }

    #[cfg(test)]
    pub fn complete_provider_execution(&mut self, result: Result<String, ProviderError>) {
        let report = result.map(|text| ProviderExecutionOutput {
            text,
            work_plan_id: String::new(),
            work_plan_manifest: String::new(),
            packet_ids: Vec::new(),
            executed_packets: 1,
            resumed_packets: 0,
        });
        self.complete_provider_execution_report(report);
    }

    pub fn complete_provider_execution_report(
        &mut self,
        result: Result<ProviderExecutionOutput, ProviderError>,
    ) {
        self.last_provider_output = match result {
            Ok(report) => match persist_provider_output(&report.text, self.provider_output_dir()) {
                Ok(evidence) => {
                    if self.runtime.mark_provider_execution_report(
                        &evidence.path.to_string_lossy(),
                        &evidence.hash,
                        evidence.len,
                        &report,
                    ) {
                        report.text
                    } else {
                        let reason = self
                            .runtime
                            .state()
                            .blockers
                            .last()
                            .cloned()
                            .unwrap_or_else(|| "provider report verification failed".to_string());
                        format!("Provider blocked: {reason}")
                    }
                }
                Err(error) => {
                    let reason = format!("provider output evidence persistence failed: {error}");
                    self.runtime.mark_provider_blocked(&reason);
                    format!("Provider blocked: {reason}")
                }
            },
            Err(error) => {
                let reason = format!("{error:?}");
                self.runtime.mark_provider_blocked(&reason);
                format!("Provider blocked: {reason}")
            }
        };
        self.provider_running = false;
        let _ = self.append_proof_record(true);
    }

    pub fn begin_design_upload_build(
        &mut self,
        ui: &UiState,
    ) -> Option<Receiver<Result<String, String>>> {
        if self.design_build_running {
            self.last_design_output = "Design upload build already running.".to_string();
            return None;
        }
        let Some(script) = design_upload_script_path() else {
            self.last_design_output =
                "Design upload blocked: scripts/Test-DesignUploadBuild.ps1 was not found."
                    .to_string();
            return None;
        };
        let html_path = ui.input_text(DESIGN_HTML_PATH_INPUT).trim().to_string();
        let css_path = ui.input_text(DESIGN_CSS_PATH_INPUT).trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.design_build_running = true;
        self.last_design_output =
            "Design upload running through the native PMRE renderer route.".to_string();
        thread::spawn(move || {
            let _ = tx.send(run_design_upload_script(script, html_path, css_path));
        });
        Some(rx)
    }

    pub fn complete_design_upload_build(&mut self, result: Result<String, String>) {
        self.design_build_running = false;
        match result {
            Ok(text) => {
                self.side_artifacts = load_side_artifacts();
                self.last_design_output = text;
                self.last_run_summary = format!(
                    "Design upload built from HTML/CSS. {}",
                    self.artifact_title_state()
                );
            }
            Err(reason) => {
                self.last_design_output = format!("Design upload blocked: {reason}");
            }
        }
    }

    #[cfg(test)]
    pub fn execute_provider(&mut self) {
        let Some(rx) = self.begin_provider_execution() else {
            return;
        };
        match rx.recv() {
            Ok(result) => self.complete_provider_execution_report(result),
            Err(error) => self.complete_provider_execution_report(Err(ProviderError::Io(format!(
                "provider worker disconnected: {error}"
            )))),
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        self.runtime.state().status
    }

    pub fn provider_title_state(&self) -> &'static str {
        if self.provider_running {
            "provider:running"
        } else if self.last_provider_output.starts_with("Provider blocked:") {
            "provider:blocked"
        } else if self.last_provider_output == "Provider has not been executed." {
            "provider:idle"
        } else {
            "provider:ran"
        }
    }

    pub fn design_title_state(&self) -> &'static str {
        if self.design_build_running {
            "design:running"
        } else if self
            .last_design_output
            .starts_with("Design upload blocked:")
        {
            "design:blocked"
        } else if self
            .last_design_output
            .lines()
            .any(|line| line.starts_with("design upload build ok:"))
        {
            "design:built"
        } else {
            "design:idle"
        }
    }

    pub fn nav_title_state(&self) -> &'static str {
        match (self.active_main_tab, self.active_settings_tab) {
            (MainTab::Workspace, _) => "assistant",
            (MainTab::Provider, _) => "provider-connections",
            (MainTab::Wiki, _) => "wiki-graph",
            (MainTab::Mcp, _) => "mcp",
            (MainTab::Skills, _) => "skills",
            (MainTab::Hooks, _) => "hooks",
            (MainTab::DesignUpload, _) => "design-upload",
            (MainTab::Settings, _) => "settings-runtime",
        }
    }

    pub fn artifact_title_state(&self) -> String {
        format!("artifacts:{}", self.side_artifacts.len())
    }

    pub fn learning_summary(&self) -> LearningSummary {
        self.learning_summary
    }

    fn append_proof_record(&mut self, learn: bool) -> StoreOutcome {
        let record = self.current_proof_record();
        let outcome = if let Some(store) = &self.store {
            match store.append(&record) {
                Ok(()) => StoreOutcome::Written,
                Err(error) => {
                    let reason = format!(
                        "Persistent proof store write failed at {}: {error}",
                        store.path().display()
                    );
                    self.runtime.mark_store_blocked(&reason);
                    return StoreOutcome::Blocked(reason);
                }
            }
        } else {
            StoreOutcome::Memory
        };
        self.runtime.learn_proof_record(&record);
        if learn {
            if let Err(reason) = self.append_learning_record() {
                self.runtime.mark_store_blocked(&reason);
                return StoreOutcome::Blocked(reason);
            }
        }
        outcome
    }

    fn append_learning_record(&mut self) -> Result<(), String> {
        let Some(record) = self.current_learning_record() else {
            return Ok(());
        };
        if let Some(store) = &self.learning_store {
            store.append(&record).map_err(|error| {
                format!(
                    "Persistent learning store write failed at {}: {error}",
                    store.path().display()
                )
            })?;
        }
        let outcome = record.outcome;
        let attempt_key = (record.intent.clone(), record.gate.clone());
        let attempt = record.attempt;
        self.runtime.learn_learning_record(&record);
        self.learning_records.push(record);
        self.learning_records =
            effective_records(&self.learning_records, DEFAULT_GRAPH_MEMORY_LIMIT);
        self.learning_summary.total += 1;
        match outcome {
            LearningOutcome::Failed => self.learning_summary.failed += 1,
            LearningOutcome::Succeeded => self.learning_summary.succeeded += 1,
        }
        self.learning_attempts.insert(attempt_key, attempt);
        Ok(())
    }

    fn current_learning_record(&self) -> Option<LearningRecord> {
        let state = self.runtime.state();
        let outcome = match state.status {
            RuntimeStatus::Proven => LearningOutcome::Succeeded,
            RuntimeStatus::Blocked => LearningOutcome::Failed,
            _ => return None,
        };
        let provider_gate = state.last_provider_call.is_some()
            || state
                .blockers
                .iter()
                .any(|blocker| blocker.to_ascii_lowercase().contains("provider"));
        let gate = if provider_gate {
            "native-provider-execution"
        } else {
            "native-proof-route"
        };
        let attempt = self
            .learning_attempts
            .get(&(self.last_intent.clone(), gate.to_string()))
            .copied()
            .unwrap_or(0)
            + 1;
        let correction = if outcome == LearningOutcome::Succeeded {
            self.learning_records
                .iter()
                .rev()
                .find(|record| {
                    record.intent == self.last_intent
                        && record.gate == gate
                        && record.outcome == LearningOutcome::Failed
                })
                .map(|record| record.failure.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let failure = if outcome == LearningOutcome::Failed {
            let blockers = state.blockers.join("; ");
            if blockers.is_empty() {
                self.last_provider_output.clone()
            } else {
                blockers
            }
        } else {
            String::new()
        };
        Some(LearningRecord::new(LearningRecordInput {
            source: "native-app".to_string(),
            intent: self.last_intent.clone(),
            recipe_id: state.selected_recipe.clone(),
            atom_stack: state.selected_atoms.clone(),
            gate: gate.to_string(),
            attempt,
            outcome,
            failure,
            correction,
            artifact_path: state.last_provider_output_artifact.clone(),
            artifact_hash: state.last_provider_output_hash.clone(),
            provider_model: state
                .last_provider_call
                .as_ref()
                .map(|call| call.model.clone())
                .unwrap_or_default(),
            work_plan_id: state.last_work_plan_id.clone(),
            work_plan_manifest: state.last_work_plan_manifest.clone(),
            work_packet_count: state.last_work_packet_count,
            harness_attestation_path: String::new(),
            harness_attestation_hash: String::new(),
            route_len: state.last_route.len(),
        }))
    }

    fn current_proof_record(&self) -> ProofRecord {
        let state = self.runtime.state();
        let provider = state.last_provider_call.as_ref();
        let provider_output_hash = if self.provider_title_state() == "provider:ran" {
            state.last_provider_output_hash.clone()
        } else {
            String::new()
        };
        ProofRecord {
            recipe_id: state.selected_recipe.clone(),
            status: state.status.as_str().to_string(),
            atoms: state.selected_atoms.clone(),
            evidence_count: state.evidence.len(),
            blockers: state.blockers.clone(),
            provider_state: self.provider_title_state().to_string(),
            provider_model: provider.map(|call| call.model.clone()).unwrap_or_default(),
            provider_endpoint: provider
                .map(|call| call.endpoint.clone())
                .unwrap_or_default(),
            provider_output_artifact: if self.provider_title_state() == "provider:ran" {
                state.last_provider_output_artifact.clone()
            } else {
                String::new()
            },
            provider_output_hash,
            provider_output_len: if self.provider_title_state() == "provider:ran" {
                state.last_provider_output_len
            } else {
                0
            },
            work_plan_id: if self.provider_title_state() == "provider:ran" {
                state.last_work_plan_id.clone()
            } else {
                String::new()
            },
            work_plan_manifest: if self.provider_title_state() == "provider:ran" {
                state.last_work_plan_manifest.clone()
            } else {
                String::new()
            },
            work_packet_count: if self.provider_title_state() == "provider:ran" {
                state.last_work_packet_count
            } else {
                0
            },
            route_len: state.last_route.len(),
        }
    }

    fn provider_output_dir(&self) -> PathBuf {
        let Some(store) = &self.store else {
            return default_provider_output_dir();
        };
        let path = store.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("proofs.jsonl"))
        {
            return path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("provider-outputs");
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("proofs");
        path.with_file_name(format!("{name}.provider-outputs"))
    }
}

fn seed_provider_inputs(provider: &ProviderConfig, ui: &mut UiState) {
    ui.inputs
        .entry(PROVIDER_KIND_INPUT)
        .or_insert_with(|| provider.kind.as_str().to_string());
    ui.inputs.entry(PROVIDER_FORMAT_INPUT).or_default();
    ui.inputs
        .entry(PROVIDER_MODEL_INPUT)
        .or_insert_with(|| provider.model.clone());
    ui.inputs
        .entry(PROVIDER_URL_INPUT)
        .or_insert_with(|| provider.endpoint.clone());
    ui.inputs
        .entry(PROVIDER_KEY_ENV_INPUT)
        .or_insert_with(|| provider.api_key_env.clone());
    ui.inputs
        .entry(PROVIDER_AUTH_HEADER_INPUT)
        .or_insert_with(|| provider.auth_header.clone());
    ui.inputs
        .entry(PROVIDER_AUTH_SCHEME_INPUT)
        .or_insert_with(|| {
            if provider.auth_scheme.is_empty() {
                "raw".to_string()
            } else {
                provider.auth_scheme.clone()
            }
        });
    ui.inputs
        .entry(PROVIDER_BODY_TEMPLATE_INPUT)
        .or_insert_with(|| provider.body_template.clone());
    ui.inputs
        .entry(PROVIDER_RESPONSE_KEY_INPUT)
        .or_insert_with(|| provider.response_key.clone());
}

fn seed_design_inputs(ui: &mut UiState) {
    ui.inputs.entry(DESIGN_HTML_PATH_INPUT).or_default();
    ui.inputs.entry(DESIGN_CSS_PATH_INPUT).or_default();
}

fn seed_lucerna_design_defaults(ui: &mut UiState) {
    ui.set_slider(DESIGN_HUE_SLIDER, 0.12);
    ui.set_slider(DESIGN_SAT_SLIDER, 0.72);
    ui.set_slider(DESIGN_LIGHT_SLIDER, 0.42);
    ui.set_slider(DESIGN_TEXT_SLIDER, 0.50);
    ui.set_slider(DESIGN_RADIUS_SLIDER, 0.26);
    ui.set_slider(DESIGN_GLASS_SLIDER, 0.24);
    ui.set_slider(DESIGN_ANIMATION_SLIDER, 0.0);
}

fn initial_provider_output(provider: &ProviderConfig) -> String {
    if provider.is_ready() {
        "Provider has not been executed.".to_string()
    } else if !provider.api_key_present {
        format!(
            "Provider blocked: Missing credential in {}",
            provider.api_key_env
        )
    } else if provider.endpoint.trim().is_empty() {
        "Provider blocked: Missing provider endpoint".to_string()
    } else if provider.model.trim().is_empty() {
        "Provider blocked: Missing provider model".to_string()
    } else {
        "Provider blocked: Provider configuration is incomplete".to_string()
    }
}

fn current_intent(ui: &UiState) -> String {
    let text = ui.input_text(INTENT_INPUT).trim();
    if text.is_empty() {
        default_intent().to_string()
    } else {
        text.to_string()
    }
}

fn execute_call(
    call: PreparedProviderCall,
) -> Result<ProviderExecutionOutput, math_atoms_core::ProviderError> {
    call.execute_with_curl_report()
}

fn run_design_upload_script(
    script: PathBuf,
    html_path: String,
    css_path: String,
) -> Result<String, String> {
    let mut command = Command::new("powershell");
    command
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script);
    if !html_path.trim().is_empty() {
        command.arg("-HtmlPath").arg(html_path.trim());
    }
    if !css_path.trim().is_empty() {
        command.arg("-CssPath").arg(css_path.trim());
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to launch design upload gate: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        if stderr.is_empty() {
            Ok(stdout)
        } else {
            Ok(format!("{stdout}\n{stderr}"))
        }
    } else {
        Err(format!(
            "design upload gate exited {}. stdout: {} stderr: {}",
            output.status, stdout, stderr
        ))
    }
}

fn design_upload_script_path() -> Option<PathBuf> {
    let script = "Test-DesignUploadBuild.ps1";
    let mut candidates = Vec::new();
    if let Ok(root) = std::env::var("MATH_ATOMS_SCRIPT_ROOT") {
        candidates.push(PathBuf::from(root).join(script));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("scripts").join(script));
        candidates.push(cwd.join("..").join("scripts").join(script));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(release_dir) = exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                if let Some(engine_dir) = target_dir.parent() {
                    candidates.push(engine_dir.join("..").join("scripts").join(script));
                }
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn load_side_artifacts() -> Vec<SideArtifact> {
    for path in artifact_manifest_candidates() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let artifacts = parse_artifact_manifest(&text);
            if !artifacts.is_empty() {
                return artifacts;
            }
        }
    }
    Vec::new()
}

fn artifact_manifest_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("MATH_ATOMS_ARTIFACT_MANIFEST") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            paths.push(PathBuf::from(trimmed));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("target/provider-built-apps/artifact-window.tsv"));
        paths.push(
            cwd.join("atom-rendering-engine-main/target/provider-built-apps/artifact-window.tsv"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(release_dir) = exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                paths.push(target_dir.join("provider-built-apps/artifact-window.tsv"));
            }
        }
    }
    paths
}

fn parse_artifact_manifest(text: &str) -> Vec<SideArtifact> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 5 || parts[0].trim().is_empty() {
                return None;
            }
            Some(SideArtifact {
                name: parts[0].trim().to_string(),
                status: parts[1].trim().to_string(),
                output: parts[2].trim().to_string(),
                source_path: parts[3].trim().to_string(),
                exe_path: parts[4].trim().to_string(),
                artifact_path: parts
                    .get(5)
                    .map(|part| part.trim())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmre_kit::ux::UxNode;
    use pmre_orchestrator::{handle_event, widget_rect, UiEvent};

    fn click_control(app: &NativeApp, ui: &mut UiState, id: u32) {
        let rect = {
            let build = |state: &UiState| crate::ui::build(app, state);
            widget_rect(&build, ui, id).expect("control should have a solved rectangle")
        };
        let x = (rect.min.x + rect.max.x) * 0.5;
        let y = (rect.min.y + rect.max.y) * 0.5;
        {
            let build = |state: &UiState| crate::ui::build(app, state);
            handle_event(ui, &build, UiEvent::PointerMove(x, y));
            handle_event(ui, &build, UiEvent::PointerDown(x, y));
            handle_event(ui, &build, UiEvent::PointerUp(x, y));
        }
        assert_eq!(ui.take_click(), Some(id));
    }

    fn tree_joined_text(node: &UxNode) -> String {
        match node {
            UxNode::Box { children, .. } => children.iter().map(tree_joined_text).collect(),
            UxNode::Text { content, .. } => content.clone(),
            UxNode::Rich { spans, .. } => spans.iter().map(|span| span.text.as_str()).collect(),
        }
    }

    fn verified_provider_report(
        app: &NativeApp,
        label: &str,
        text: &str,
    ) -> (ProviderExecutionOutput, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-work-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = math_atoms_core::WorkPlanStore::new(&root);
        let call = app.runtime.state().last_provider_call.as_ref().unwrap();
        let mut plan = call.work_plan.clone().unwrap();
        plan.expand_files(vec![math_atoms_core::WorkFile {
            path: "response.txt".to_string(),
            purpose: "provider response".to_string(),
            acceptance: vec!["response verified".to_string()],
        }])
        .unwrap();
        let lease = store.acquire(&plan.id).unwrap();
        let manifest = store.write_plan_manifest(&plan).unwrap();
        for packet in &plan.packets {
            let output = match packet.contract {
                math_atoms_core::PacketContract::Envelope => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"complete\",\"checks\":[\"verified\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_core::PacketContract::FileManifest => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"response.txt\",\"purpose\":\"provider response\",\"acceptance\":[\"response verified\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_core::PacketContract::FileArtifact => {
                    "```text\nprovider proof\n```".to_string()
                }
            };
            store
                .store_packet(&plan, packet, &output, &call.model)
                .unwrap();
        }
        drop(lease);
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
            },
            root,
        )
    }

    #[test]
    fn native_run_uses_core_bus_and_provider_readiness() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        assert_eq!(ui.slider_value(DESIGN_ANIMATION_SLIDER, 1.0), 0.0);
        app.run_current_intent(&ui);
        assert_eq!(app.status(), RuntimeStatus::ProviderPending);
        assert!(app.runtime.bus().contains_all_layers());
        assert!(app.runtime.state().last_provider_call.is_some());
    }

    #[test]
    fn native_run_blocks_without_provider_key() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        assert_eq!(app.status(), RuntimeStatus::Blocked);
        assert!(app
            .runtime
            .state()
            .blockers
            .iter()
            .any(|item| item.contains("OPENAI_API_KEY")));
    }

    #[test]
    fn provider_title_state_tracks_execution_state() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        assert_eq!(app.provider_title_state(), "provider:idle");
        app.last_provider_output = "Provider blocked: MissingApiKey".to_string();
        assert_eq!(app.provider_title_state(), "provider:blocked");
        app.last_provider_output = "model response".to_string();
        assert_eq!(app.provider_title_state(), "provider:ran");
    }

    #[test]
    fn design_title_state_tracks_build_state() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        assert_eq!(app.design_title_state(), "design:idle");
        app.design_build_running = true;
        assert_eq!(app.design_title_state(), "design:running");
        app.design_build_running = false;
        app.last_design_output = "Design upload blocked: missing html".to_string();
        assert_eq!(app.design_title_state(), "design:blocked");
        app.last_design_output =
            "design upload build ok: MATH_ATOMS_DESIGN_APP_OK uploaded-design-app html=1 css=1 bmp=design-upload-app.bmp"
                .to_string();
        assert_eq!(app.design_title_state(), "design:built");
        app.last_design_output = format!(
            "MATH_ATOMS_LEARNING_OK id=test outcome=succeeded total=1\n{}",
            app.last_design_output
        );
        assert_eq!(app.design_title_state(), "design:built");
    }

    #[test]
    fn missing_provider_starts_blocked_not_idle() {
        let app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        assert_eq!(app.provider_title_state(), "provider:blocked");
        assert!(app.last_provider_output.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn provider_button_without_prepared_call_fails_closed() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        app.execute_provider();
        assert_eq!(app.provider_title_state(), "provider:blocked");
        assert_eq!(app.status(), RuntimeStatus::Blocked);
        assert!(app
            .runtime
            .state()
            .blockers
            .iter()
            .any(|item| item.contains("No prepared provider call")));
    }

    #[test]
    fn provider_completion_failure_blocks_runtime() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        app.provider_running = true;
        app.complete_provider_execution(Err(ProviderError::Io("provider failed".to_string())));
        assert_eq!(app.status(), RuntimeStatus::Blocked);
        assert_eq!(app.provider_title_state(), "provider:blocked");
    }

    #[test]
    fn provider_setup_inputs_apply_to_runtime() {
        let key = format!("MATH_ATOMS_NATIVE_UI_KEY_{}", std::process::id());
        std::env::set_var(&key, "secret");
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        ui.inputs
            .insert(PROVIDER_KIND_INPUT, "ollama-cloud".to_string());
        ui.inputs
            .insert(PROVIDER_MODEL_INPUT, "gpt-oss:120b".to_string());
        ui.inputs.insert(
            PROVIDER_URL_INPUT,
            "https://ollama.com/api/chat".to_string(),
        );
        ui.inputs.insert(PROVIDER_KEY_ENV_INPUT, key.clone());
        app.apply_provider_config(&ui);
        std::env::remove_var(&key);
        assert_eq!(app.runtime.provider().kind.as_str(), "ollama");
        assert_eq!(app.runtime.provider().model, "gpt-oss:120b");
        assert!(app.runtime.provider().api_key_present);
        assert_eq!(app.status(), RuntimeStatus::Draft);
        assert_eq!(app.provider_title_state(), "provider:idle");
        assert!(app.last_run_summary.contains("Provider config applied"));
    }

    #[test]
    fn provider_connections_tab_owns_provider_controls() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let ui = UiState::new(1200, 800);

        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            assert!(widget_rect(&build, &ui, SETTINGS_TAB).is_some());
            assert!(widget_rect(&build, &ui, PROVIDER_CONNECTIONS_TAB).is_some());
            assert!(widget_rect(&build, &ui, WIKI_TAB).is_some());
            assert!(widget_rect(&build, &ui, MCP_TAB).is_some());
            assert!(widget_rect(&build, &ui, SKILLS_TAB).is_some());
            assert!(widget_rect(&build, &ui, HOOKS_TAB).is_some());
            assert!(widget_rect(&build, &ui, PROVIDER_KIND_INPUT).is_none());
        }

        app.show_settings();
        app.show_runtime_settings();
        assert_eq!(app.nav_title_state(), "settings-runtime");
        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            assert!(widget_rect(&build, &ui, PROVIDER_CONNECTIONS_TAB).is_some());
            assert!(widget_rect(&build, &ui, DESIGN_UPLOAD_TAB).is_some());
            assert!(widget_rect(&build, &ui, RUNTIME_SETTINGS_TAB).is_some());
            assert!(widget_rect(&build, &ui, PROVIDER_KIND_INPUT).is_none());
        }

        app.show_provider_connections();
        assert_eq!(app.nav_title_state(), "provider-connections");
        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            assert!(widget_rect(&build, &ui, PROVIDER_KIND_INPUT).is_some());
            assert!(widget_rect(&build, &ui, PROVIDER_BODY_TEMPLATE_INPUT).is_some());
            assert!(widget_rect(&build, &ui, APPLY_PROVIDER).is_some());
        }
    }

    #[test]
    fn settings_design_upload_tab_owns_design_controls() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let ui = UiState::new(1200, 800);

        app.show_design_upload();
        assert_eq!(app.nav_title_state(), "design-upload");
        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            assert!(widget_rect(&build, &ui, DESIGN_HTML_PATH_INPUT).is_some());
            assert!(widget_rect(&build, &ui, DESIGN_CSS_PATH_INPUT).is_some());
            assert!(widget_rect(&build, &ui, BUILD_DESIGN_UPLOAD).is_some());
            assert!(widget_rect(&build, &ui, PROVIDER_KIND_INPUT).is_none());
        }
    }

    #[test]
    fn app_shell_scrolls_at_small_viewports() {
        let app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(900, 520);
        app.seed_input(&mut ui);

        let build = |state: &UiState| crate::ui::build(&app, state);
        handle_event(&mut ui, &build, UiEvent::Wheel(4.0, 4.0, 120.0));

        assert!(ui.scroll_of(APP_SCROLL) > 0.0);
        assert!(widget_rect(&build, &ui, SETTINGS_TAB).is_some());
    }

    #[test]
    fn focused_chat_box_renders_a_visible_caret() {
        let app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        ui.inputs.insert(INTENT_INPUT, "abc".to_string());
        ui.focused = Some(INTENT_INPUT);
        ui.input_carets.insert(INTENT_INPUT, 1);
        ui.animation_time = 0.0;

        let tree = crate::ui::build(&app, &ui);
        assert!(tree_joined_text(&tree).contains("a|bc"));

        ui.focused = None;
        let tree = crate::ui::build(&app, &ui);
        assert!(!tree_joined_text(&tree).contains("a|bc"));
    }

    #[test]
    fn side_artifact_manifest_loads_generated_apps() {
        let manifest = "name\tstatus\toutput\tsource\texe\tartifact\ncounter\tcompiled\tMATH_ATOMS_APP_OK counter total=4\tC:\\src\\counter.rs\tC:\\bin\\counter.exe\t\nrouter\tcompiled\tMATH_ATOMS_APP_OK router health=200 atoms=3\tC:\\src\\router.rs\tC:\\bin\\router.exe\tC:\\artifacts\\router.bmp\n";
        let artifacts = parse_artifact_manifest(manifest);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].name, "counter");
        assert_eq!(artifacts[0].status, "compiled");
        assert_eq!(
            artifacts[1].output,
            "MATH_ATOMS_APP_OK router health=200 atoms=3"
        );
        assert_eq!(artifacts[1].artifact_path, "C:\\artifacts\\router.bmp");
    }

    #[test]
    fn side_artifact_window_is_visible_from_manifest() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-artifact-window-{}-{}.tsv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "name\tstatus\toutput\tsource\texe\ncounter\tcompiled\tMATH_ATOMS_APP_OK counter total=4\tC:\\src\\counter.rs\tC:\\bin\\counter.exe\n",
        )
        .unwrap();
        std::env::set_var("MATH_ATOMS_ARTIFACT_MANIFEST", &path);
        let app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        std::env::remove_var("MATH_ATOMS_ARTIFACT_MANIFEST");
        std::fs::remove_file(&path).ok();

        assert_eq!(app.side_artifacts.len(), 1);
        assert_eq!(app.artifact_title_state(), "artifacts:1");
        let ui = UiState::new(1200, 800);
        let build = |state: &UiState| crate::ui::build(&app, state);
        assert!(widget_rect(&build, &ui, ARTIFACT_SCROLL).is_some());
    }

    #[test]
    fn visible_controls_hit_test_and_dispatch() {
        let key = format!("MATH_ATOMS_VISIBLE_CONTROL_KEY_{}", std::process::id());
        std::env::set_var(&key, "secret");
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let mut ui = UiState::new(1600, 1000);
        app.seed_input(&mut ui);
        ui.inputs.insert(PROVIDER_KEY_ENV_INPUT, key.clone());
        ui.inputs.insert(
            PROVIDER_URL_INPUT,
            "http://127.0.0.1:9/v1/responses".to_string(),
        );

        click_control(&app, &mut ui, SETTINGS_TAB);
        app.show_settings();
        assert_eq!(app.active_main_tab, MainTab::Settings);

        click_control(&app, &mut ui, PROVIDER_CONNECTIONS_TAB);
        app.show_provider_connections();
        assert_eq!(app.active_main_tab, MainTab::Provider);
        assert_eq!(app.active_settings_tab, SettingsTab::ProviderConnections);

        click_control(&app, &mut ui, APPLY_PROVIDER);
        app.apply_provider_config(&ui);
        assert_eq!(app.status(), RuntimeStatus::Draft);

        click_control(&app, &mut ui, WORKSPACE_TAB);
        app.show_workspace();
        assert_eq!(app.active_main_tab, MainTab::Workspace);

        click_control(&app, &mut ui, RUN_LOOP);
        app.run_current_intent(&ui);
        assert!(!app.runtime.state().last_route.is_empty());

        click_control(&app, &mut ui, CAPTURE_PROOF);
        app.capture_current_proof();
        assert!(app
            .last_run_summary
            .contains("Capture blocked: provider execution must complete"));

        click_control(&app, &mut ui, EXEC_PROVIDER);
        let rx = app
            .begin_provider_execution()
            .expect("provider-ready route should start execution");
        assert!(app
            .runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == math_atoms_core::BusMessageKind::ProviderExecutionScheduled));
        assert!(app
            .runtime
            .bus()
            .route_contains_all_layers(*app.runtime.state().last_route.last().unwrap()));
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("local refused provider should return quickly");
        app.complete_provider_execution_report(result);
        std::env::remove_var(&key);
        assert_eq!(app.provider_title_state(), "provider:blocked");

        click_control(&app, &mut ui, MARK_DRIFT);
        app.mark_drift();
        assert_eq!(app.status(), RuntimeStatus::DriftFlagged);
    }

    #[test]
    fn native_run_writes_store_when_configured() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-native-store-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let learning_path = LearningStore::beside(&path).path().to_path_buf();
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(learning_path).ok();
        assert!(text.contains("\"status\":\"provider pending\""));
        assert!(text.contains("\"recipe_id\":"));
    }

    #[test]
    fn native_run_blocks_when_persistent_store_fails() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-native-store-dir-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let store = ProofStore::new(&path);
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        std::fs::remove_dir_all(&path).ok();
        assert_eq!(app.status(), RuntimeStatus::Blocked);
        assert!(app.last_run_summary.starts_with("Blocked:"));
        assert!(app
            .runtime
            .state()
            .blockers
            .iter()
            .any(|item| item.contains("Persistent proof store write failed")));
        assert!(app
            .runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == math_atoms_core::BusMessageKind::StoreBlocked));
    }

    #[test]
    fn provider_execution_state_is_persisted() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-provider-store-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let learning_path = LearningStore::beside(&path).path().to_path_buf();
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        assert_eq!(app.status(), RuntimeStatus::ProviderPending);
        app.runtime.mark_provider_blocked("test");
        app.last_provider_output = "Provider blocked: test".to_string();
        let _ = app.append_proof_record(true);
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(learning_path).ok();
        assert!(text.contains("\"status\":\"blocked\""));
        assert!(text.contains("\"provider_state\":\"provider:blocked\""));
    }

    #[test]
    fn provider_execution_success_persists_output_audit() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-provider-audit-store-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let learning_path = LearningStore::beside(&path).path().to_path_buf();
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let output_dir = app.provider_output_dir();
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        app.provider_running = true;
        let (report, work_root) = verified_provider_report(&app, "output-audit", "provider proof");
        app.complete_provider_execution_report(Ok(report));
        assert_eq!(app.status(), RuntimeStatus::VerificationPending);
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(learning_path).ok();
        std::fs::remove_dir_all(output_dir).ok();
        std::fs::remove_dir_all(work_root).ok();
        assert!(text.contains("\"provider_state\":\"provider:ran\""));
        assert!(text.contains("\"provider_model\":"));
        assert!(text.contains("\"provider_output_artifact\":"));
        assert!(text.contains("\"provider_output_hash\":\"sha256:"));
        assert!(text.contains("\"provider_output_len\":14"));
        assert!(app
            .runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == math_atoms_core::BusMessageKind::ProviderExecuted));
    }

    #[test]
    fn capture_button_writes_an_additional_proof_record() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-capture-store-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let learning_path = LearningStore::beside(&path).path().to_path_buf();
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let output_dir = app.provider_output_dir();
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        let (report, work_root) = verified_provider_report(&app, "capture", "provider proof");
        app.complete_provider_execution_report(Ok(report));
        let before = store.read_records().unwrap().len();
        app.capture_current_proof();
        let after = store.read_records().unwrap().len();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(learning_path).ok();
        std::fs::remove_dir_all(output_dir).ok();
        std::fs::remove_dir_all(work_root).ok();
        assert_eq!(after, before + 1);
        assert!(app
            .last_run_summary
            .contains("Captured current proof route"));
    }

    #[test]
    fn failed_attempt_is_retrieved_after_restart_and_corrected() {
        let proof_path = std::env::temp_dir().join(format!(
            "math-atoms-learning-restart-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let learning_path = LearningStore::beside(&proof_path).path().to_path_buf();
        let proofs = ProofStore::new(&proof_path);
        let learning = LearningStore::new(&learning_path);
        let mut first = NativeApp::new_with_stores(
            ProviderConfig::from_pairs(&[]),
            Some(proofs.clone()),
            Some(learning.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        first.seed_input(&mut ui);
        first.run_current_intent(&ui);
        assert_eq!(first.learning_summary().failed, 1);
        assert!(first.runtime.bus().envelopes().iter().any(|envelope| {
            envelope.kind == math_atoms_core::BusMessageKind::LearningPersisted
        }));
        assert!(first
            .runtime
            .bus()
            .route_contains_all_layers(*first.runtime.state().last_route.last().unwrap()));
        drop(first);

        let mut restarted = NativeApp::new_with_stores(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(proofs),
            Some(learning.clone()),
        );
        let output_dir = restarted.provider_output_dir();
        restarted.seed_input(&mut ui);
        restarted.run_current_intent(&ui);
        assert!(restarted
            .runtime
            .state()
            .evidence
            .iter()
            .any(|item| item.node_id.starts_with("learning:failed:")));
        let (report, work_root) = verified_provider_report(
            &restarted,
            "restart-correction",
            "corrected provider output",
        );
        restarted.complete_provider_execution_report(Ok(report));
        let records = learning.read_records().unwrap();
        assert_eq!(LearningSummary::from_records(&records).failed, 1);
        assert_eq!(LearningSummary::from_records(&records).succeeded, 0);
        assert_eq!(restarted.status(), RuntimeStatus::VerificationPending);
        let before_capture = records.len();
        restarted.capture_current_proof();
        assert_eq!(learning.read_records().unwrap().len(), before_capture);
        std::fs::remove_file(proof_path).ok();
        std::fs::remove_file(learning_path).ok();
        std::fs::remove_dir_all(output_dir).ok();
        std::fs::remove_dir_all(work_root).ok();
    }
}
