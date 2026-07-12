use atom_vibe_build_protocol::BuildStep;
use atom_vibe_context::CoderTurnContext;
use atom_vibe_provider::{ProviderAdapterError, ProviderRequest, ProviderTurnReceipt};
use math_atoms_core::ProviderConfig;
use std::fmt;
use std::path::{Path, PathBuf};

pub const SESSION_SCHEMA_VERSION: u32 = 1;
pub const TURN_RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub planners: PathBuf,
    pub sessions: PathBuf,
    pub scratchpads: PathBuf,
    pub outputs: PathBuf,
    pub turns: PathBuf,
}

impl RuntimePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            planners: root.join("planners"),
            sessions: root.join("sessions"),
            scratchpads: root.join("scratchpads"),
            outputs: root.join("outputs"),
            turns: root.join("turns"),
            root,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionManifest {
    pub schema_version: u32,
    pub build_id: String,
    pub project_id: String,
    pub operator_request: String,
    pub initial_provider: String,
    pub initial_model: String,
    pub initial_provider_identity_hash: String,
    pub created_at_unix_ms: u64,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedTurn {
    pub build_id: String,
    pub step: BuildStep,
    pub planner_revision: u64,
    pub provider_identity_hash: String,
    pub context: CoderTurnContext,
    pub request: ProviderRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderResultRoute {
    pub terminal: u64,
    pub route: Vec<u64>,
    pub blocked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnRecord {
    pub schema_version: u32,
    pub ordinal: u64,
    pub build_id: String,
    pub step: BuildStep,
    pub planner_revision: u64,
    pub provider: String,
    pub model: String,
    pub request_body_hash: String,
    pub raw_response_hash: String,
    pub output_hash: String,
    pub output_artifact: String,
    pub elapsed_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub thinking_source: String,
    pub evidence_ids: Vec<String>,
    pub context_route: Vec<u64>,
    pub result_route: Vec<u64>,
    pub scratchpad_entry_hash: String,
    pub previous_hash: String,
    pub record_hash: String,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutedTurn {
    pub receipt: ProviderTurnReceipt,
    pub output_artifact: PathBuf,
    pub result_route: ProviderResultRoute,
    pub record: TurnRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    InvalidConfiguration(String),
    InvalidRequest(String),
    Session(String),
    Planner(String),
    Context(String),
    Mode(String),
    Scratchpad(String),
    Provider(ProviderAdapterError),
    TurnStore(String),
    BuildNotActive(String),
    StalePreparedTurn,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(reason) => {
                write!(output, "invalid Atom Vibe runtime configuration: {reason}")
            }
            Self::InvalidRequest(reason) => write!(output, "invalid build request: {reason}"),
            Self::Session(reason) => write!(output, "build session failed: {reason}"),
            Self::Planner(reason) => write!(output, "build planner failed: {reason}"),
            Self::Context(reason) => write!(output, "coder context failed: {reason}"),
            Self::Mode(reason) => write!(output, "coder mode failed: {reason}"),
            Self::Scratchpad(reason) => write!(output, "model scratchpad failed: {reason}"),
            Self::Provider(error) => write!(output, "provider turn failed: {error}"),
            Self::TurnStore(reason) => write!(output, "provider turn store failed: {reason}"),
            Self::BuildNotActive(build_id) => write!(output, "build {build_id} is not active"),
            Self::StalePreparedTurn => {
                output.write_str("prepared provider turn is stale and cannot execute")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<ProviderAdapterError> for RuntimeError {
    fn from(error: ProviderAdapterError) -> Self {
        Self::Provider(error)
    }
}

pub(crate) fn provider_identity(config: &ProviderConfig) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}",
        config.kind.as_str(),
        config.wire_format.as_str(),
        config.endpoint,
        config.model,
        config
            .thinking_level
            .map(|level| level.as_str())
            .unwrap_or("invalid")
    )
}

pub(crate) fn safe_relative(path: &Path) -> Option<String> {
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(value) = component else {
            return None;
        };
        let value = value.to_str()?;
        if value.is_empty() || value.chars().any(char::is_control) {
            return None;
        }
        parts.push(value);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
