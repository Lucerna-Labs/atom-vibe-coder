use atom_vibe_native_bridge::{default_runtime_root, NativeVibe, VibeWorkerResult};
use math_atoms_core::{
    default_provider_output_dir, effective_records, persist_provider_output, LearningOutcome,
    LearningRecord, LearningStore, LearningSummary, MathAtomsRuntime, PreparedProviderCall,
    ProofRecord, ProofStore, ProviderConfig, ProviderConfigInput, ProviderError,
    ProviderExecutionOutput, RuntimeStatus, WikiGraph, DEFAULT_GRAPH_MEMORY_LIMIT,
};
use pmre_orchestrator::{
    UiState, DESIGN_ANIMATION_SLIDER, DESIGN_GAMMA_SLIDER, DESIGN_GLASS_SLIDER, DESIGN_HUE_SLIDER,
    DESIGN_LIGHT_SLIDER, DESIGN_RADIUS_SLIDER, DESIGN_SAT_SLIDER, DESIGN_TEXT_SLIDER,
};
use std::collections::HashMap;
use std::path::PathBuf;
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
pub const PROVIDER_THINKING_INPUT: u32 = 34;
pub const EXEC_VIBE_STEP: u32 = 35;
pub const INTENT_INPUT_SCROLL: u32 = 36;
pub const TRANSCRIPT_SCROLL: u32 = 37;
pub const PROVIDER_OUTPUT_SCROLL: u32 = 38;

pub fn default_intent() -> &'static str {
    "Build a native Atom Vibe Coder app on the Spiderweb Bus with provider API, wiki graph RAG, proof capture, and side artifact preview."
}

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
    pub fast_build_running: bool,
    pub design_build_running: bool,
    pub last_design_output: String,
    pub active_main_tab: MainTab,
    pub active_settings_tab: SettingsTab,
    pub side_artifacts: Vec<SideArtifact>,
    pub vibe: NativeVibe,
    last_intent: String,
}

/// Built-app row for the side-artifacts pane. The type and its manifest/build helpers
/// live in `avc-core` (the vibe-build crate); re-exported here under the name the UI and
/// tests already use.
pub use avc_core::BuildArtifact as SideArtifact;

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
        let provider = ProviderConfig::from_process_env();
        let vibe = NativeVibe::open(default_runtime_root(), provider.clone());
        Self::new_with_stores(
            provider,
            Some(ProofStore::new(ProofStore::default_path())),
            Some(LearningStore::new(LearningStore::default_path())),
            vibe,
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
        Self::new_with_stores(
            provider,
            store,
            learning_store,
            NativeVibe::unavailable("disabled in unit-test constructor"),
        )
    }

    pub fn new_with_stores(
        provider: ProviderConfig,
        store: Option<ProofStore>,
        learning_store: Option<LearningStore>,
        vibe: NativeVibe,
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
            fast_build_running: false,
            design_build_running: false,
            last_design_output: "Design upload has not been built.".to_string(),
            active_main_tab: MainTab::Workspace,
            active_settings_tab: SettingsTab::ProviderConnections,
            side_artifacts: avc_core::load_artifacts(),
            vibe,
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
        let proof = self.runtime.run_coder_intent(&intent);
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
        match self.vibe.start_build(&intent) {
            Ok(()) => {
                self.last_run_summary.push_str(" Vibe session prepared.");
            }
            Err(error) => {
                self.last_run_summary
                    .push_str(&format!(" Vibe session blocked: {error}"));
            }
        }
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
            thinking_level: ui.input_text(PROVIDER_THINKING_INPUT),
        });
        let ready = config.is_ready();
        let provider_output = initial_provider_output(&config);
        let readiness = if ready {
            "ready"
        } else if !config.api_key_present {
            "key missing"
        } else if config.thinking_level.is_none() {
            "thinking invalid"
        } else {
            "incomplete"
        };
        let summary = format!(
            "Provider config applied: {} {} {} with {} thinking via {} ({})",
            config.kind.as_str(),
            config.wire_format.as_str(),
            config.model,
            config
                .thinking_level
                .map(math_atoms_core::ProviderThinkingLevel::as_str)
                .unwrap_or("invalid"),
            config.endpoint,
            readiness
        );
        self.runtime.set_provider(config.clone());
        if let Err(error) = self.vibe.set_provider(config) {
            self.last_run_summary = format!("{summary}. Vibe provider blocked: {error}");
        } else {
            self.last_run_summary = summary;
        }
        self.last_provider_output = provider_output;
        self.provider_running = false;
    }

    pub fn begin_vibe_step(&mut self) -> Option<Receiver<VibeWorkerResult>> {
        self.vibe.begin_step()
    }

    pub fn complete_vibe_step(&mut self, result: VibeWorkerResult) {
        self.vibe.complete_step(result);
        self.last_run_summary = self.vibe.summary().to_string();
    }

    pub fn vibe_worker_disconnected(&mut self) {
        self.vibe.worker_disconnected();
        self.last_run_summary = self.vibe.summary().to_string();
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

    /// Fast single-shot build (vendored from the v1 Atom Vibe Coder): one prompt, one
    /// provider call, extract the fenced code. This is what the Run button uses instead
    /// of the slow 9-packet work plan. Returns a receiver the UI polls; the worker runs
    /// on its own thread so the renderer stays responsive.
    pub fn begin_fast_build(
        &mut self,
        ui: &UiState,
    ) -> Option<Receiver<Result<avc_core::FastBuild, String>>> {
        if self.fast_build_running {
            self.last_provider_output = "Fast build already running.".to_string();
            return None;
        }
        let intent = current_intent(ui);
        // Bug #1 fix (wrong-intent ledger writes): the fast-build path never touched
        // `self.last_intent`, so `complete_fast_build` was calling
        // `record_fast_build_learning(&self.last_intent, ...)` with whatever intent the
        // SLOW mission path last processed — usually the boot-time mission intent.
        // Every notebook/orchestration/kernel failure was therefore written to the
        // learning ledger + wiki-graph tagged with the wrong intent tokens, and the
        // NEXT same-intent Run's `graph.retrieve` couldn't find its own prior failure.
        // Mirror the slow path (`run_current_intent`), which sets `last_intent` before
        // the run begins.
        self.last_intent = intent.clone();
        // Structured blueprint for THIS run (C1: pass directly, don't wait for
        // scratchpad round-trip). Also persisted as a Decision so a future Run's
        // prepare_turn projects it back — per-run scratchpad memory, distinct from the
        // cross-run wiki-graph learning ledger (H5).
        let blueprint = {
            let state = self.runtime.state();
            avc_core::format_build_blueprint(&intent, &state.selected_recipe, &state.selected_atoms)
        };
        if let Some(warn) = math_atoms_native_ledger::warn_if_memory_lost(
            self.vibe.append_planner_blueprint(&blueprint),
            "blueprint",
        ) {
            self.last_provider_output.push_str(&warn);
        }
        // Bug #2 fix (stale evidence): `state.evidence` is populated only by the SLOW
        // `run_intent_in_mode` path. On the Run button, that field held evidence for
        // whatever intent last hit the slow path (often the boot-time mission), NOT
        // the current fast-build intent — so `learning:failed:*` records for the
        // current intent from prior Runs never reached the model. Re-query the graph
        // here with the ACTUAL intent so cross-Run learning works.
        let fresh_evidence = self.runtime.retrieve_evidence(&intent);
        let evidence: Vec<(String, String)> = fresh_evidence
            .iter()
            .take(6)
            .map(|item| (item.title.clone(), item.excerpt.clone()))
            .collect();
        // Prior failed-build lessons specifically. `run_fast_build`'s internal rewrite
        // loop can't itself hit the graph (it runs on a worker thread with no runtime
        // handle), so we pre-fetch the top few `learning:failed:*` records for this
        // intent NOW and pass them in; the rewrite prompt then names them explicitly
        // as "avoid this pattern" alongside the fresh rustc errors. Learning within a
        // SINGLE Run (round 2 seeing round 1's failure) still requires the threading
        // refactor and is captured in `nat-review-backlog` — this pass unblocks the
        // cross-Run half, which is what the operator asked for.
        let selected_recipe = self.runtime.state().selected_recipe.clone();
        let selected_atoms = self.runtime.state().selected_atoms.clone();
        let scratchpad_memory = self.vibe.scratchpad_projection().unwrap_or("").to_string();
        let prior_lessons: Vec<(String, String)> = fresh_evidence
            .iter()
            .filter(|item| item.node_id.starts_with("learning:failed:"))
            .take(4)
            .map(|item| (item.title.clone(), item.excerpt.clone()))
            .collect();
        let budget = std::env::var("VIBE_CONTEXT_BUDGET_CHARS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(16000);
        let plan = avc_core::build_fast_plan(
            &selected_recipe,
            &selected_atoms,
            &evidence,
            &intent,
            &blueprint,
            &scratchpad_memory,
            budget,
        );
        // C2: use the UI-applied provider (via runtime), not process env.
        let config = math_atoms_native_ledger::provider_config_to_avc(self.runtime.provider());
        if let Err(error) = config.prepare_build_call(&intent, &plan) {
            self.last_provider_output = format!("Provider blocked: {error}");
            return None;
        }
        let dir = avc_core::fast_build_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        self.fast_build_running = true;
        self.last_provider_output =
            "Fast build running through a single provider call.".to_string();
        // OPERATOR_APPROVED_CPU_PARALLEL: I/O worker thread for the blocking curl call,
        // identical to the existing begin_provider_execution / begin_vibe_step workers in
        // this file — keeps the renderer responsive, not added compute parallelism.
        thread::spawn(move || {
            let _ = tx.send(avc_core::run_fast_build(
                &config,
                &intent,
                &plan,
                &dir,
                stamp,
                &prior_lessons,
            ));
        });
        Some(rx)
    }

    pub fn complete_fast_build(&mut self, result: Result<avc_core::FastBuild, String>) {
        self.fast_build_running = false;
        match result {
            Ok(build) => {
                let name = build.artifact.name.clone();
                let bytes = build.bytes;
                let path = build.artifact.source_path.clone();
                // NAT-review fix: `compile_check` runs `rustc --emit=metadata` (typeck +
                // borrowck only, no codegen/monomorphization) -- "typechecks" is what was
                // actually verified; "compiles" overstated it.
                let verdict = if !build.verified {
                    "unverified (no rustc)".to_string()
                } else if build.compiled {
                    if build.repair_attempts == 0 {
                        "typechecks".to_string()
                    } else {
                        format!("typechecks after {} repair(s)", build.repair_attempts)
                    }
                } else {
                    format!("still failing after {} repair(s)", build.repair_attempts)
                };
                let mut output = format!(
                    "Built {name} ({bytes} bytes, {verdict}) -> {path}\n\n{}",
                    build.preview
                );
                if build.verified && !build.compiled && !build.compile_errors.is_empty() {
                    output.push_str("\n\n--- remaining rustc errors ---\n");
                    output.push_str(&build.compile_errors);
                }
                self.last_provider_output = output;
                self.last_run_summary = format!("Fast build {verdict}: {name} ({bytes} bytes).");
                // H6: surface silent scratchpad failures. Per-run memory only; cross-run
                // memory is the wiki-graph learning ledger below (H5).
                if let Some(warn) = math_atoms_native_ledger::warn_if_memory_lost(
                    self.vibe.append_fast_build_outcome(&build),
                    "outcome",
                ) {
                    self.last_provider_output.push_str(&warn);
                }
                let fallback_model = self.runtime.provider().model.clone();
                math_atoms_native_ledger::record_fast_build_learning(
                    &mut self.runtime,
                    &self.last_intent,
                    &fallback_model,
                    self.learning_store.as_ref(),
                    &mut self.learning_records,
                    &mut self.learning_summary,
                    &mut self.learning_attempts,
                    &build,
                );
                self.side_artifacts.insert(0, build.artifact);
            }
            Err(reason) => {
                self.last_provider_output = format!("Provider blocked: {reason}");
                self.last_run_summary = format!("Fast build blocked: {reason}");
                if let Some(warn) = math_atoms_native_ledger::warn_if_memory_lost(
                    self.vibe.append_fast_build_blocked(&reason),
                    "block",
                ) {
                    self.last_provider_output.push_str(&warn);
                }
            }
        }
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
            candidate_verification: None,
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
        let Some(script) = avc_core::design_upload_script_path() else {
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
            let _ = tx.send(avc_core::run_design_upload_script(
                script, html_path, css_path,
            ));
        });
        Some(rx)
    }

    pub fn complete_design_upload_build(&mut self, result: Result<String, String>) {
        self.design_build_running = false;
        match result {
            Ok(text) => {
                self.side_artifacts = avc_core::load_artifacts();
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
        if self.provider_running || self.fast_build_running {
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
        math_atoms_native_ledger::learning_record_from_state(
            self.runtime.state(),
            &self.last_intent,
            &self.learning_attempts,
            &self.learning_records,
            &self.last_provider_output,
        )
    }

    fn current_proof_record(&self) -> ProofRecord {
        math_atoms_native_ledger::proof_record_from_state(
            self.runtime.state(),
            self.provider_title_state(),
        )
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
    ui.inputs.entry(PROVIDER_THINKING_INPUT).or_insert_with(|| {
        provider
            .thinking_level
            .map(math_atoms_core::ProviderThinkingLevel::as_str)
            .unwrap_or("invalid")
            .to_string()
    });
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
    ui.set_slider(DESIGN_GAMMA_SLIDER, 0.25);
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
    } else if provider.thinking_level.is_none() {
        "Provider blocked: Thinking must be low, medium, or high".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use pmre_kit::ux::UxNode;
    use pmre_orchestrator::{handle_event, widget_rect, UiEvent};

    const VERIFIED_PROVIDER_SOURCE: &str =
        "pub fn provider_proof() -> &'static str { \"provider proof\" }\n";
    const CORRECTED_PROVIDER_SOURCE: &str =
        "pub fn corrected_provider_output() -> &'static str { \"corrected\" }\n";

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
            path: "src/lib.rs".to_string(),
            purpose: "provider response".to_string(),
            acceptance: vec!["crate checks, tests, and lints cleanly".to_string()],
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
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"src/lib.rs\",\"purpose\":\"provider response\",\"acceptance\":[\"crate checks, tests, and lints cleanly\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_core::PacketContract::FileArtifact => format!("```rust\n{text}```"),
            };
            store
                .store_packet(&plan, packet, &output, &call.model)
                .unwrap();
        }
        drop(lease);
        let verifier = math_atoms_core::CandidateVerifier::new(
            &root,
            math_atoms_core::VerificationPolicy::strict(120).unwrap(),
        );
        let candidate = vec![math_atoms_core::CandidateFile::new("src/lib.rs", text).unwrap()];
        let attempt = verifier.verify_attempt(&plan.id, 1, &candidate).unwrap();
        assert!(attempt.passed, "{}", attempt.failure);
        let verification = verifier.finalize(&attempt).unwrap();
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
                candidate_verification: Some(math_atoms_core::CandidateVerificationReport {
                    manifest_path: verification.manifest_path.to_string_lossy().to_string(),
                    manifest_hash: verification.manifest_hash,
                    bundle_hash: verification.bundle_hash,
                    attempts: verification.attempts,
                    repairs: verification.repairs,
                }),
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
        assert_eq!(ui.slider_value(DESIGN_GAMMA_SLIDER, 0.0), 0.25);
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
        assert_eq!(
            app.runtime
                .provider()
                .thinking_level
                .map(math_atoms_core::ProviderThinkingLevel::as_str),
            Some("low")
        );
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
            assert!(widget_rect(&build, &ui, PROVIDER_THINKING_INPUT).is_some());
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
        let artifacts = avc_core::parse_artifact_manifest(manifest);
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
        let (report, work_root) =
            verified_provider_report(&app, "output-audit", VERIFIED_PROVIDER_SOURCE);
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
        assert!(text.contains("\"candidate_verification\":{\"manifest_path\""));
        assert!(text.contains(&format!(
            "\"provider_output_len\":{}",
            VERIFIED_PROVIDER_SOURCE.len()
        )));
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
        let (report, work_root) =
            verified_provider_report(&app, "capture", VERIFIED_PROVIDER_SOURCE);
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
            NativeVibe::unavailable("disabled in learning restart test"),
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
            NativeVibe::unavailable("disabled in learning restart test"),
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
        let (report, work_root) =
            verified_provider_report(&restarted, "restart-correction", CORRECTED_PROVIDER_SOURCE);
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

    #[test]
    fn fast_build_learns_from_failures_but_never_fabricates_successes() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-fastbuild-learn-{}-{}.jsonl",
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
            Some(store),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        // Select the recipe + retrieve Wiki Graph evidence into the state the record reads.
        app.run_current_intent(&ui);

        let make = |compiled: bool, errors: &str| avc_core::FastBuild {
            artifact: avc_core::BuildArtifact {
                name: "vibe-build-test".to_string(),
                status: if compiled { "typechecks" } else { "errors" }.to_string(),
                output: "MATH_ATOMS_APP".to_string(),
                source_path: "vibe-build-test.rs".to_string(),
                exe_path: String::new(),
                artifact_path: "vibe-build-test.rs".to_string(),
            },
            bytes: 128,
            preview: "fn main() {}".to_string(),
            verified: true,
            compiled,
            repair_attempts: 2,
            compile_errors: errors.to_string(),
        };

        // A compile failure is recorded as a reusable "correct this" lesson.
        let before_failed = app.learning_summary.failed;
        app.complete_fast_build(Ok(make(
            false,
            "error[E0277]: `AtomMeasure` doesn't implement `Debug`",
        )));
        assert_eq!(app.learning_summary.failed, before_failed + 1);

        // A compiling build must NOT fabricate a provider-grade success record.
        let before_total = app.learning_summary.total;
        app.complete_fast_build(Ok(make(true, "")));
        assert_eq!(app.learning_summary.total, before_total);

        let learned = LearningStore::new(&learning_path)
            .read_records()
            .unwrap_or_default();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&learning_path).ok();
        assert!(
            learned
                .iter()
                .any(|record| record.gate == "native-fast-build"
                    && record.outcome == LearningOutcome::Failed
                    && record.failure.contains("E0277")),
            "the persisted lesson should carry the fast-build gate and the rustc error"
        );
    }

    #[test]
    #[ignore = "manual visual dump for UI verification"]
    fn dump_visual_frames_for_manual_inspection() {
        let out_dir = std::path::PathBuf::from(
            std::env::var("MATH_ATOMS_VISUAL_DUMP_DIR").unwrap_or_else(|_| ".".to_string()),
        );
        let app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1240, 820);
        app.seed_input(&mut ui);
        let long_text = "this is a very long chat message typed by the operator to stress the chat box wrapping so it keeps going and going with plenty of words and then one giant unbreakable token Supercalifragilisticexpialidocious-Supercalifragilisticexpialidocious-Supercalifragilisticexpialidocious and then a bit more prose after the long token to prove the flow recovers cleanly and continues wrapping like normal text should";
        ui.inputs.insert(INTENT_INPUT, long_text.to_string());
        ui.input_carets
            .insert(INTENT_INPUT, long_text.chars().count());
        ui.focused = Some(INTENT_INPUT);
        ui.scrolls.insert(crate::model::INTENT_INPUT_SCROLL, 1.0e9);
        let build = |state: &UiState| crate::ui::build(&app, state);
        ui.animation_time = 0.0; // caret visible
        let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
        std::fs::write(
            out_dir.join("chatbox-long-text-caret-on.bmp"),
            fb.to_bmp(crate::ui::background()),
        )
        .unwrap();
        ui.animation_time = 0.5; // caret hidden
        let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
        std::fs::write(
            out_dir.join("chatbox-long-text-caret-off.bmp"),
            fb.to_bmp(crate::ui::background()),
        )
        .unwrap();
        ui.animation_time = 0.0;
        ui.width = 1240;
        ui.height = 1000;
        let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
        std::fs::write(
            out_dir.join("chatbox-full-height.bmp"),
            fb.to_bmp(crate::ui::background()),
        )
        .unwrap();
        ui.inputs.insert(INTENT_INPUT, "hello world".to_string());
        ui.input_carets.insert(INTENT_INPUT, 0);
        ui.scrolls.insert(crate::model::INTENT_INPUT_SCROLL, 0.0);
        let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
        std::fs::write(
            out_dir.join("chatbox-caret-at-zero.bmp"),
            fb.to_bmp(crate::ui::background()),
        )
        .unwrap();
        ui.inputs.insert(INTENT_INPUT, String::new());
        ui.input_carets.insert(INTENT_INPUT, 0);
        let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
        std::fs::write(
            out_dir.join("chatbox-empty-input.bmp"),
            fb.to_bmp(crate::ui::background()),
        )
        .unwrap();
    }

    #[test]
    fn rapid_backspace_clear_never_panics_the_render_loop() {
        let app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1240, 820);
        app.seed_input(&mut ui);
        let build = |state: &UiState| crate::ui::build(&app, state);
        for step in 0..200 {
            handle_event(&mut ui, &build, UiEvent::Backspace);
            // mimic the caret-follow scroll pin and the blink clock
            ui.scrolls.insert(INTENT_INPUT_SCROLL, 1.0e9);
            ui.animation_time = (ui.animation_time + 0.033).rem_euclid(3600.0);
            let fb = pmre_orchestrator::render_ui(&build, &ui, crate::ui::background());
            assert!(!fb.pixels().is_empty(), "frame {step} rendered no pixels");
        }
        assert_eq!(ui.input_text(INTENT_INPUT), "");
    }

    #[test]
    fn clicking_the_chat_input_focuses_it_despite_its_inner_scroll_region() {
        let app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        let mut ui = UiState::new(1600, 1000);
        app.seed_input(&mut ui);
        ui.focused = None;

        let rect = {
            let build = |state: &UiState| crate::ui::build(&app, state);
            widget_rect(&build, &ui, INTENT_INPUT).expect("chat input is laid out")
        };
        // the inner clip/scroll region must exist so long text cannot paint over the buttons
        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            assert!(widget_rect(&build, &ui, INTENT_INPUT_SCROLL).is_some());
        }
        let x = (rect.min.x + rect.max.x) * 0.5;
        let y = (rect.min.y + rect.max.y) * 0.5;
        {
            let build = |state: &UiState| crate::ui::build(&app, state);
            handle_event(&mut ui, &build, UiEvent::PointerDown(x, y));
            handle_event(&mut ui, &build, UiEvent::PointerUp(x, y));
        }
        assert_eq!(ui.focused, Some(INTENT_INPUT));
    }
}
