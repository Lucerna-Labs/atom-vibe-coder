use math_atoms_core::{
    MathAtomsRuntime, PreparedProviderCall, ProofRecord, ProofStore, ProviderConfig, RuntimeStatus,
};
use pmre_orchestrator::UiState;

pub const INTENT_INPUT: u32 = 1;
pub const RUN_LOOP: u32 = 2;
pub const EXEC_PROVIDER: u32 = 3;
pub const CAPTURE_PROOF: u32 = 4;
pub const MARK_DRIFT: u32 = 5;
pub const EVIDENCE_SCROLL: u32 = 6;
pub const BUS_SCROLL: u32 = 7;

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
        ui.focused = Some(INTENT_INPUT);
    }

    pub fn run_current_intent(&mut self, ui: &UiState) {
        let intent = current_intent(ui);
        let proof = self.runtime.run_intent(&intent);
        let store_result = self.append_proof_record();
        self.last_run_summary = if proof.blockers.is_empty() {
            format!(
                "{} proven with {} evidence nodes through {} route envelopes. {}",
                proof.recipe_id,
                proof.evidence.len(),
                self.runtime.state().last_route.len(),
                store_result
            )
        } else {
            format!("Blocked: {} {}", proof.blockers.join("; "), store_result)
        };
    }

    pub fn mark_drift(&mut self) {
        self.runtime
            .mark_drift("Operator flagged drift from the native PMRE shell.");
        self.last_run_summary =
            "Drift flagged; next proof run must re-establish evidence.".to_string();
    }

    pub fn capture_current_proof(&mut self) {
        if self.runtime.state().last_route.is_empty() {
            self.last_run_summary =
                "Capture blocked: run a proof route before storing evidence.".to_string();
            return;
        }
        let store_result = self.append_proof_record();
        self.last_run_summary = format!("Captured current proof route. {store_result}");
    }

    pub fn execute_provider(&mut self) {
        let Some(call) = self.runtime.state().last_provider_call.clone() else {
            let reason =
                "No prepared provider call. Run an intent that requests provider/model work first.";
            self.runtime.mark_provider_blocked(reason);
            self.last_provider_output = format!("Provider blocked: {reason}");
            return;
        };
        self.provider_running = true;
        self.last_provider_output = match execute_call(call) {
            Ok(text) => text,
            Err(error) => {
                let reason = format!("{error:?}");
                self.runtime.mark_provider_blocked(&reason);
                format!("Provider blocked: {reason}")
            }
        };
        self.provider_running = false;
        let _ = self.append_proof_record();
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

    fn append_proof_record(&mut self) -> &'static str {
        let record = self.current_proof_record();
        self.runtime.learn_proof_record(&record);
        let Some(store) = &self.store else {
            return "store:memory";
        };
        match store.append(&record) {
            Ok(()) => "store:written",
            Err(_) => "store:blocked",
        }
    }

    fn current_proof_record(&self) -> ProofRecord {
        let state = self.runtime.state();
        ProofRecord {
            recipe_id: state.selected_recipe.clone(),
            status: state.status.as_str().to_string(),
            atoms: state.selected_atoms.clone(),
            evidence_count: state.evidence.len(),
            blockers: state.blockers.clone(),
            provider_state: self.provider_title_state().to_string(),
            route_len: state.last_route.len(),
        }
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

fn execute_call(call: PreparedProviderCall) -> Result<String, math_atoms_core::ProviderError> {
    call.execute_with_curl()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_run_uses_core_bus_and_provider_readiness() {
        let mut app = NativeApp::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let mut ui = UiState::new(1200, 800);
        app.seed_input(&mut ui);
        app.run_current_intent(&ui);
        assert_eq!(app.status(), RuntimeStatus::Proven);
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
        assert!(text.contains("\"status\":\"proven\""));
        assert!(text.contains("\"recipe_id\":"));
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
        app.runtime.mark_provider_blocked("test");
        app.last_provider_output = "Provider blocked: test".to_string();
        let _ = app.append_proof_record();
        let text = store.read_to_string().unwrap();
        std::fs::remove_file(&path).ok();
        assert!(text.contains("\"status\":\"blocked\""));
        assert!(text.contains("\"provider_state\":\"provider:blocked\""));
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
