use math_atoms_core::{
    provider_output_hash, MathAtomsRuntime, PreparedProviderCall, ProofRecord, ProofStore,
    ProviderConfig, ProviderConfigInput, ProviderError, RuntimeStatus,
};
use pmre_orchestrator::UiState;
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
pub const LEFT_SCROLL: u32 = 13;
pub const PROVIDER_FORMAT_INPUT: u32 = 14;
pub const PROVIDER_AUTH_HEADER_INPUT: u32 = 15;
pub const PROVIDER_AUTH_SCHEME_INPUT: u32 = 16;
pub const PROVIDER_BODY_TEMPLATE_INPUT: u32 = 17;
pub const PROVIDER_RESPONSE_KEY_INPUT: u32 = 18;

pub fn default_intent() -> &'static str {
    "Build the native atom-rendered Math Atoms Coder on the Spiderweb Bus with provider API, wiki graph RAG, proof capture, and Ornith 1.0 parity."
}

#[derive(Clone, Debug)]
pub struct NativeApp {
    pub runtime: MathAtomsRuntime,
    pub store: Option<ProofStore>,
    pub last_run_summary: String,
    pub last_provider_output: String,
    pub provider_running: bool,
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
        Self::new_with_store(
            ProviderConfig::from_process_env(),
            Some(ProofStore::new(ProofStore::default_path())),
        )
    }

    #[cfg(test)]
    pub fn new(provider: ProviderConfig) -> Self {
        Self::new_with_store(provider, None)
    }

    pub fn new_with_store(provider: ProviderConfig, store: Option<ProofStore>) -> Self {
        Self {
            runtime: MathAtomsRuntime::new(provider),
            store,
            last_run_summary: "No proof run yet.".to_string(),
            last_provider_output: "Provider has not been executed.".to_string(),
            provider_running: false,
        }
    }

    pub fn seed_input(&self, ui: &mut UiState) {
        ui.inputs
            .entry(INTENT_INPUT)
            .or_insert_with(|| default_intent().to_string());
        seed_provider_inputs(self.runtime.provider(), ui);
        ui.focused = Some(INTENT_INPUT);
    }

    pub fn run_current_intent(&mut self, ui: &UiState) {
        let intent = current_intent(ui);
        let proof = self.runtime.run_intent(&intent);
        let store_result = self.append_proof_record();
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
        let store_result = self.append_proof_record();
        self.last_run_summary = if matches!(store_result, StoreOutcome::Blocked(_)) {
            format!("Capture blocked: {}", store_result.message())
        } else {
            format!("Captured current proof route. {}", store_result.message())
        };
    }

    pub fn begin_provider_execution(&mut self) -> Option<Receiver<Result<String, ProviderError>>> {
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

    pub fn complete_provider_execution(&mut self, result: Result<String, ProviderError>) {
        self.last_provider_output = match result {
            Ok(text) => {
                let output_hash = provider_output_hash(&text);
                self.runtime
                    .mark_provider_executed(&output_hash, text.len());
                text
            }
            Err(error) => {
                let reason = format!("{error:?}");
                self.runtime.mark_provider_blocked(&reason);
                format!("Provider blocked: {reason}")
            }
        };
        self.provider_running = false;
        let _ = self.append_proof_record();
    }

    #[cfg(test)]
    pub fn execute_provider(&mut self) {
        let Some(rx) = self.begin_provider_execution() else {
            return;
        };
        match rx.recv() {
            Ok(result) => self.complete_provider_execution(result),
            Err(error) => self.complete_provider_execution(Err(ProviderError::Io(format!(
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

    fn append_proof_record(&mut self) -> StoreOutcome {
        let record = self.current_proof_record();
        let Some(store) = &self.store else {
            self.runtime.learn_proof_record(&record);
            return StoreOutcome::Memory;
        };
        match store.append(&record) {
            Ok(()) => {
                self.runtime.learn_proof_record(&record);
                StoreOutcome::Written
            }
            Err(error) => {
                let reason = format!(
                    "Persistent proof store write failed at {}: {error}",
                    store.path().display()
                );
                self.runtime.mark_store_blocked(&reason);
                StoreOutcome::Blocked(reason)
            }
        }
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
            provider_output_hash,
            provider_output_len: if self.provider_title_state() == "provider:ran" {
                state.last_provider_output_len
            } else {
                0
            },
            route_len: state.last_route.len(),
        }
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

fn current_intent(ui: &UiState) -> String {
    let text = ui.input_text(INTENT_INPUT).trim();
    if text.is_empty() {
        default_intent().to_string()
    } else {
        text.to_string()
    }
}

fn execute_call(call: PreparedProviderCall) -> Result<String, math_atoms_core::ProviderError> {
    call.execute_with_curl()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn native_run_uses_core_bus_and_provider_readiness() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
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
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[]));
        assert_eq!(app.provider_title_state(), "provider:idle");
        app.last_provider_output = "Provider blocked: MissingApiKey".to_string();
        assert_eq!(app.provider_title_state(), "provider:blocked");
        app.last_provider_output = "model response".to_string();
        assert_eq!(app.provider_title_state(), "provider:ran");
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

        ui.scrolls.insert(LEFT_SCROLL, 500.0);
        click_control(&app, &mut ui, APPLY_PROVIDER);
        app.apply_provider_config(&ui);
        assert_eq!(app.status(), RuntimeStatus::Draft);

        ui.scrolls.insert(LEFT_SCROLL, 0.0);
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
        app.complete_provider_execution(result);
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
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
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
        let _ = app.append_proof_record();
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
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
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        app.provider_running = true;
        app.complete_provider_execution(Ok("provider proof".to_string()));
        assert_eq!(app.status(), RuntimeStatus::Proven);
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
        assert!(text.contains("\"provider_state\":\"provider:ran\""));
        assert!(text.contains("\"provider_model\":"));
        assert!(text.contains("\"provider_output_hash\":\"fnv:"));
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
        let mut app = NativeApp::new_with_store(
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]),
            Some(store.clone()),
        );
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        app.complete_provider_execution(Ok("provider proof".to_string()));
        let before = store.read_records().unwrap().len();
        app.capture_current_proof();
        let after = store.read_records().unwrap().len();
        std::fs::remove_file(&path).ok();
        assert_eq!(after, before + 1);
        assert!(app
            .last_run_summary
            .contains("Captured current proof route"));
    }
}
