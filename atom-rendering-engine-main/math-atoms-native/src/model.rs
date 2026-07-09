use math_atoms_core::{MathAtomsRuntime, PreparedProviderCall, ProviderConfig, RuntimeStatus};
use pmre_orchestrator::UiState;

pub const INTENT_INPUT: u32 = 1;
pub const RUN_LOOP: u32 = 2;
pub const EXEC_PROVIDER: u32 = 3;
pub const MARK_DRIFT: u32 = 4;
pub const EVIDENCE_SCROLL: u32 = 5;
pub const BUS_SCROLL: u32 = 6;

pub fn default_intent() -> &'static str {
    "Build the native atom-rendered Math Atoms Coder on the Spiderweb Bus with provider API, wiki graph RAG, proof capture, and Ornith 1.0 parity."
}

#[derive(Clone, Debug)]
pub struct NativeApp {
    pub runtime: MathAtomsRuntime,
    pub last_run_summary: String,
    pub last_provider_output: String,
    pub provider_running: bool,
}

impl NativeApp {
    pub fn from_process_env() -> Self {
        Self::new(ProviderConfig::from_process_env())
    }

    pub fn new(provider: ProviderConfig) -> Self {
        Self {
            runtime: MathAtomsRuntime::new(provider),
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
        self.last_run_summary = if proof.blockers.is_empty() {
            format!(
                "{} proven with {} evidence nodes through {} route envelopes.",
                proof.recipe_id,
                proof.evidence.len(),
                self.runtime.state().last_route.len()
            )
        } else {
            format!("Blocked: {}", proof.blockers.join("; "))
        };
    }

    pub fn mark_drift(&mut self) {
        self.runtime
            .mark_drift("Operator flagged drift from the native PMRE shell.");
        self.last_run_summary =
            "Drift flagged; next proof run must re-establish evidence.".to_string();
    }

    pub fn execute_provider(&mut self) {
        let Some(call) = self.runtime.state().last_provider_call.clone() else {
            self.last_provider_output =
                "No prepared provider call. Run an intent that requests provider/model work first."
                    .to_string();
            return;
        };
        self.provider_running = true;
        self.last_provider_output = match execute_call(call) {
            Ok(text) => text,
            Err(error) => format!("Provider blocked: {error:?}"),
        };
        self.provider_running = false;
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
}
