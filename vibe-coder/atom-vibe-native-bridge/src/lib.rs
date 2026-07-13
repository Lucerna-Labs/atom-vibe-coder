//! Native PMRE bridge for the durable Atom Vibe build runtime.

use atom_vibe_runtime::{AtomVibeRuntime, ExecutedTurn, PreparedTurn, RuntimeError};
pub use atom_vibe_scratchpad::ScratchpadEntryKind;
use math_atoms_core::ProviderConfig;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

const NATIVE_PROJECT_ID: &str = "native-atom-vibe-coder";

pub struct VibeWorkerResult {
    runtime: AtomVibeRuntime,
    result: Result<ExecutedTurn, RuntimeError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VibeState {
    Unavailable,
    Idle,
    Prepared,
    Running,
    VerificationPending,
    Blocked,
}

pub struct NativeVibe {
    runtime: Option<AtomVibeRuntime>,
    /// Durable runtime root, kept even when open fails so a later provider
    /// change can reopen the runtime instead of leaving the subsystem dead.
    root: Option<PathBuf>,
    prepared: Option<PreparedTurn>,
    active_build_id: Option<String>,
    current_step: Option<String>,
    context_route_len: usize,
    result_route_len: usize,
    turn_count: usize,
    state: VibeState,
    detail: String,
}

impl NativeVibe {
    pub fn open(root: PathBuf, provider: ProviderConfig) -> Self {
        match AtomVibeRuntime::open(root.clone(), provider) {
            Ok(runtime) => Self {
                runtime: Some(runtime),
                root: Some(root),
                prepared: None,
                active_build_id: None,
                current_step: None,
                context_route_len: 0,
                result_route_len: 0,
                turn_count: 0,
                state: VibeState::Idle,
                detail: "Ready to start a durable six-stage build.".to_string(),
            },
            Err(error) => {
                let mut vibe = Self::unavailable(error.to_string());
                vibe.root = Some(root);
                vibe
            }
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            runtime: None,
            root: None,
            prepared: None,
            active_build_id: None,
            current_step: None,
            context_route_len: 0,
            result_route_len: 0,
            turn_count: 0,
            state: VibeState::Unavailable,
            detail: reason.into(),
        }
    }

    pub fn start_build(&mut self, operator_request: &str) -> Result<(), String> {
        let runtime = self
            .runtime
            .as_mut()
            .ok_or_else(|| "Atom Vibe runtime is unavailable".to_string())?;
        let session = runtime
            .start_build(NATIVE_PROJECT_ID, operator_request)
            .map_err(|error| error.to_string())?;
        let prepared = runtime
            .prepare_turn(&session.build_id)
            .map_err(|error| error.to_string())?;
        self.context_route_len = prepared.context.route.route.len();
        self.current_step = Some(prepared.step.as_str().to_string());
        self.active_build_id = Some(session.build_id.clone());
        self.prepared = Some(prepared);
        self.result_route_len = 0;
        self.turn_count = 0;
        self.state = VibeState::Prepared;
        self.detail = format!(
            "Build {} prepared {} through {} context envelopes.",
            session.build_id,
            self.current_step.as_deref().unwrap_or("unknown"),
            self.context_route_len
        );
        Ok(())
    }

    pub fn begin_step(&mut self) -> Option<Receiver<VibeWorkerResult>> {
        if self.state == VibeState::Running {
            self.detail = "A Vibe step is already running.".to_string();
            return None;
        }
        let Some(runtime) = self.runtime.take() else {
            self.state = VibeState::Blocked;
            self.detail = "Atom Vibe runtime is unavailable.".to_string();
            return None;
        };
        let Some(prepared) = self.prepared.take() else {
            self.runtime = Some(runtime);
            if self.state == VibeState::VerificationPending {
                // A completed step is not a failure: it is holding for the deterministic
                // build-gate verification harness (real warning-denied command evidence,
                // independent review, and launch proof), which is a separate subsystem.
                // Report that honestly instead of a misleading "run a request first".
                self.detail = "Step output persisted; holding for deterministic gate \
                    verification before the next stage."
                    .to_string();
            } else {
                self.state = VibeState::Blocked;
                self.detail = "Run an operator request before executing a Vibe step.".to_string();
            }
            return None;
        };
        let (tx, rx) = mpsc::channel();
        self.state = VibeState::Running;
        self.detail = format!(
            "Executing {} through the thinking-required provider route.",
            prepared.step.as_str()
        );
        thread::spawn(move || {
            let mut runtime = runtime;
            let result = runtime.execute_turn(&prepared);
            let _ = tx.send(VibeWorkerResult { runtime, result });
        });
        Some(rx)
    }

    pub fn complete_step(&mut self, worker: VibeWorkerResult) {
        self.runtime = Some(worker.runtime);
        match worker.result {
            Ok(executed) => {
                self.result_route_len = executed.result_route.route.len();
                self.turn_count = executed.record.ordinal as usize;
                self.current_step = Some(executed.record.step.as_str().to_string());
                self.state = VibeState::VerificationPending;
                self.detail = format!(
                    "{} output persisted as {} with {} result-route envelopes; deterministic gate verification is pending.",
                    executed.record.step.as_str(),
                    executed.output_artifact.display(),
                    self.result_route_len
                );
            }
            Err(error) => {
                self.state = VibeState::Blocked;
                self.detail = format!("Vibe step blocked: {error}");
            }
        }
    }

    pub fn worker_disconnected(&mut self) {
        self.runtime = None;
        self.state = VibeState::Blocked;
        self.detail = "Vibe provider worker disconnected; apply a provider to reopen the runtime."
            .to_string();
    }

    /// The current scratchpad PROJECTION for the active build — the text `prepare_turn`
    /// packed for the model (operator request + prior stage notes + prior corrections,
    /// budget-clamped). Callers inject this into the generation prompt so persistent
    /// memory is actually READ, not merely stored.
    pub fn scratchpad_projection(&self) -> Option<&str> {
        self.prepared
            .as_ref()
            .map(|prepared| prepared.context.scratchpad.text.as_str())
    }

    /// Append a persistent-memory note to the active build's scratchpad. The next
    /// `prepare_turn` will project it back into the prompt, closing the read/write loop.
    /// Returns Ok(false) when there is no active build (fail-open, not an error).
    pub fn append_scratchpad_note(
        &self,
        kind: ScratchpadEntryKind,
        content: &str,
        source_ids: &[String],
    ) -> Result<bool, String> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(false);
        };
        let Some(build_id) = self.active_build_id.as_deref() else {
            return Ok(false);
        };
        runtime
            .append_scratchpad_note(build_id, kind, content, source_ids)
            .map(|_| true)
            .map_err(|error| error.to_string())
    }

    /// Persist a planner-first blueprint as a `Decision` scratchpad entry BEFORE code
    /// generation. Called by Run to record the intent + recipe + atom stack as the plan.
    pub fn append_planner_blueprint(&self, blueprint: &str) -> Result<bool, String> {
        self.append_scratchpad_note(
            ScratchpadEntryKind::Decision,
            blueprint,
            &["native:planner-first-blueprint".to_string()],
        )
    }

    /// Persist a fast-build outcome as scratchpad memory: `GateFailure` when rustc-verified
    /// but failing, `PacketOutput` otherwise. The next stage or rerun projects it back.
    pub fn append_fast_build_outcome(&self, build: &avc_core::FastBuild) -> Result<bool, String> {
        let name = &build.artifact.name;
        let (kind, note) = if build.verified && !build.compiled {
            (
                ScratchpadEntryKind::GateFailure,
                format!(
                    "Fast build {name} FAILED rustc after {} repair(s). Errors:\n{}",
                    build.repair_attempts, build.compile_errors
                ),
            )
        } else {
            let verdict = if build.verified && build.compiled {
                if build.repair_attempts == 0 {
                    "compiles".to_string()
                } else {
                    format!("compiles after {} repair(s)", build.repair_attempts)
                }
            } else {
                "unverified".to_string()
            };
            (
                ScratchpadEntryKind::PacketOutput,
                format!(
                    "Fast build {name}: {verdict}, {} bytes -> {}",
                    build.bytes, build.artifact.source_path
                ),
            )
        };
        self.append_scratchpad_note(kind, &note, &["native:fast-build-outcome".to_string()])
    }

    /// Record that a fast build was blocked before it even reached the model.
    pub fn append_fast_build_blocked(&self, reason: &str) -> Result<bool, String> {
        self.append_scratchpad_note(
            ScratchpadEntryKind::GateFailure,
            &format!("Fast build blocked before generation: {reason}"),
            &["native:fast-build-blocked".to_string()],
        )
    }

    pub fn set_provider(&mut self, provider: ProviderConfig) -> Result<(), String> {
        if self.state == VibeState::Running {
            return Err("Vibe provider cannot change while a step is running".to_string());
        }
        match self.runtime.as_mut() {
            Some(runtime) => runtime
                .set_provider(provider)
                .map_err(|error| error.to_string())?,
            None => {
                // The runtime never opened (e.g. no credential at launch) or its
                // worker died; reopen it with the new provider instead of staying dead.
                let root = self
                    .root
                    .clone()
                    .ok_or_else(|| format!("Atom Vibe runtime is unavailable: {}", self.detail))?;
                let runtime =
                    AtomVibeRuntime::open(root, provider).map_err(|error| error.to_string())?;
                self.runtime = Some(runtime);
            }
        }
        self.prepared = None;
        self.active_build_id = None;
        self.current_step = None;
        self.context_route_len = 0;
        self.result_route_len = 0;
        self.turn_count = 0;
        self.state = VibeState::Idle;
        self.detail = "Provider changed; start a new durable build session.".to_string();
        Ok(())
    }

    pub fn title_state(&self) -> &'static str {
        match self.state {
            VibeState::Unavailable => "vibe:unavailable",
            VibeState::Idle => "vibe:idle",
            VibeState::Prepared => "vibe:prepared",
            VibeState::Running => "vibe:running",
            VibeState::VerificationPending => "vibe:verification-pending",
            VibeState::Blocked => "vibe:blocked",
        }
    }

    pub fn summary(&self) -> &str {
        &self.detail
    }

    pub fn active_build_id(&self) -> &str {
        self.active_build_id.as_deref().unwrap_or("none")
    }

    pub fn current_step(&self) -> &str {
        self.current_step.as_deref().unwrap_or("none")
    }

    pub fn context_route_len(&self) -> usize {
        self.context_route_len
    }

    pub fn result_route_len(&self) -> usize {
        self.result_route_len
    }

    pub fn turn_count(&self) -> usize {
        self.turn_count
    }
}

pub fn default_runtime_root() -> PathBuf {
    if let Some(path) = non_empty_env("MATH_ATOMS_VIBE_RUNTIME_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = non_empty_env("MATH_ATOMS_STORE_DIR") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("vibe-runtime");
    }
    if let Some(path) = non_empty_env("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("LucernaLabs")
            .join("MathAtomsCoder")
            .join("vibe-runtime");
    }
    std::env::temp_dir()
        .join("LucernaLabs")
        .join("MathAtomsCoder")
        .join("vibe-runtime")
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }

    #[test]
    fn native_start_creates_a_durable_intake_session_over_full_context_route() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-vibe-{}-{}",
            std::process::id(),
            stamp()
        ));
        let provider = ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "configured")]);
        let mut vibe = NativeVibe::open(root.clone(), provider);
        vibe.start_build("Build a native inventory app").unwrap();
        assert_eq!(vibe.title_state(), "vibe:prepared");
        assert_eq!(vibe.current_step(), "intake");
        assert!(vibe.active_build_id().starts_with("build-"));
        assert_eq!(vibe.context_route_len(), 4);
        assert!(root
            .join("sessions")
            .join(format!("{}.json", vibe.active_build_id()))
            .is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_step_reports_gate_verification_not_a_missing_request() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-vibe-verify-{}-{}",
            std::process::id(),
            stamp()
        ));
        let mut vibe = NativeVibe::open(
            root.clone(),
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "cfg")]),
        );
        vibe.start_build("Build a native inventory app").unwrap();
        // Simulate a step that executed and persisted its output: the runtime is back,
        // the prepared turn is consumed, and the state is holding for verification.
        vibe.state = VibeState::VerificationPending;
        vibe.prepared = None;
        assert!(vibe.begin_step().is_none());
        // It must NOT regress to a misleading "run a request first" / blocked state.
        assert_eq!(vibe.title_state(), "vibe:verification-pending");
        assert!(
            vibe.summary().contains("gate verification"),
            "unexpected detail: {}",
            vibe.summary()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn provider_apply_reopens_an_unavailable_runtime() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-vibe-reopen-{}-{}",
            std::process::id(),
            stamp()
        ));
        // No credential at launch: the runtime fails to open and the bridge is dead.
        let mut vibe = NativeVibe::open(root.clone(), ProviderConfig::from_pairs(&[]));
        assert_eq!(vibe.title_state(), "vibe:unavailable");
        // Applying a working provider must reopen the runtime instead of erroring.
        vibe.set_provider(ProviderConfig::from_pairs(&[(
            "OPENAI_API_KEY",
            "configured",
        )]))
        .unwrap();
        assert_eq!(vibe.title_state(), "vibe:idle");
        vibe.start_build("Recover the vibe runtime").unwrap();
        assert_eq!(vibe.title_state(), "vibe:prepared");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn native_vibe_scratchpad_write_loop_persists_observation_blueprint_and_outcome() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-vibe-scratchpad-{}-{}",
            std::process::id(),
            stamp()
        ));
        let mut vibe = NativeVibe::open(
            root.clone(),
            ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "configured")]),
        );
        // start_build writes an Observation entry (operator request).
        vibe.start_build("Build a stack calculator with error handling")
            .unwrap();
        let build_id = vibe.active_build_id().to_string();
        assert!(build_id.starts_with("build-"));

        // append_planner_blueprint writes a Decision entry.
        assert_eq!(
            vibe.append_planner_blueprint("STRUCTURED BUILD BLUEPRINT: test plan")
                .unwrap(),
            true
        );
        // append_fast_build_outcome with a compiled build writes a PacketOutput.
        let compiled = avc_core::FastBuild {
            artifact: avc_core::BuildArtifact {
                name: "vibe-build-ok".to_string(),
                status: "compiles".to_string(),
                output: "MATH_ATOMS_APP_OK".to_string(),
                source_path: "vibe-build-ok.rs".to_string(),
                exe_path: String::new(),
                artifact_path: "vibe-build-ok.rs".to_string(),
            },
            bytes: 64,
            preview: "fn main() {}".to_string(),
            verified: true,
            compiled: true,
            repair_attempts: 0,
            compile_errors: String::new(),
        };
        assert_eq!(vibe.append_fast_build_outcome(&compiled).unwrap(), true);
        // A rustc-failing build writes a GateFailure.
        let failed = avc_core::FastBuild {
            compiled: false,
            compile_errors: "error[E0277]: `Foo` doesn't implement `Debug`".to_string(),
            ..compiled.clone()
        };
        assert_eq!(vibe.append_fast_build_outcome(&failed).unwrap(), true);

        // Walk on-disk entries: root/scratchpads/<build_id>/<hash>/entries/*.json
        let scope_dir = root.join("scratchpads").join(&build_id);
        let hash_dir = std::fs::read_dir(&scope_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let entries_dir = hash_dir.join("entries");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&entries_dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        files.sort();
        let bodies: Vec<String> = files
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(bodies.len(), 4, "one observation + blueprint + 2 outcomes");
        assert!(bodies[0].contains("\"kind\":\"observation\""));
        assert!(bodies[0].contains("Build a stack calculator"));
        assert!(bodies[1].contains("\"kind\":\"decision\""));
        assert!(bodies[1].contains("STRUCTURED BUILD BLUEPRINT"));
        assert!(bodies[2].contains("\"kind\":\"packet_output\""));
        assert!(bodies[2].contains("vibe-build-ok"));
        assert!(bodies[3].contains("\"kind\":\"gate_failure\""));
        assert!(bodies[3].contains("E0277"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn provider_change_invalidates_prepared_native_turn() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-native-vibe-provider-{}-{}",
            std::process::id(),
            stamp()
        ));
        let first = ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "configured")]);
        let mut vibe = NativeVibe::open(root.clone(), first);
        vibe.start_build("Build a native notes app").unwrap();
        let next = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "ollama"),
            ("MATH_ATOMS_PROVIDER_MODEL", "qwen3.5:9b"),
            ("OLLAMA_API_KEY", "configured"),
        ]);
        vibe.set_provider(next).unwrap();
        assert_eq!(vibe.title_state(), "vibe:idle");
        assert_eq!(vibe.active_build_id(), "none");
        assert!(vibe.begin_step().is_none());
        assert_eq!(vibe.title_state(), "vibe:blocked");
        std::fs::remove_dir_all(root).unwrap();
    }
}
