use math_atoms_provider_transport::ProviderTransportError;
use std::fmt;

pub const MAX_PROVIDER_INSTRUCTIONS_BYTES: usize = 512 * 1024;
pub const MAX_PROVIDER_DATA_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRequest {
    pub request_id: String,
    pub system_instructions: String,
    pub data: String,
    pub max_output_bytes: usize,
}

impl ProviderRequest {
    pub fn new(
        request_id: impl Into<String>,
        system_instructions: impl Into<String>,
        data: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            system_instructions: system_instructions.into(),
            data: data.into(),
            max_output_bytes: math_atoms_provider_transport::MAX_PROVIDER_OUTPUT_BYTES,
        }
    }

    pub fn validate(&self) -> Result<(), ProviderAdapterError> {
        if self.request_id.trim() != self.request_id
            || self.request_id.is_empty()
            || self.request_id.len() > 160
            || self
                .request_id
                .chars()
                .any(|ch| ch.is_control() || !(ch.is_ascii_alphanumeric() || "-_.:".contains(ch)))
        {
            return Err(ProviderAdapterError::InvalidRequest(
                "request id is empty, unsafe, or exceeds 160 bytes".to_string(),
            ));
        }
        if self.system_instructions.trim().is_empty()
            || self.system_instructions.len() > MAX_PROVIDER_INSTRUCTIONS_BYTES
        {
            return Err(ProviderAdapterError::InvalidRequest(format!(
                "system instructions must be nonempty and no larger than {MAX_PROVIDER_INSTRUCTIONS_BYTES} bytes"
            )));
        }
        if self.data.trim().is_empty() || self.data.len() > MAX_PROVIDER_DATA_BYTES {
            return Err(ProviderAdapterError::InvalidRequest(format!(
                "provider data must be nonempty and no larger than {MAX_PROVIDER_DATA_BYTES} bytes"
            )));
        }
        if !(1..=math_atoms_provider_transport::MAX_PROVIDER_OUTPUT_BYTES)
            .contains(&self.max_output_bytes)
        {
            return Err(ProviderAdapterError::InvalidRequest(
                "provider output limit is outside the transport bounds".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThinkingEvidence {
    pub source: String,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderTurnReceipt {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub text: String,
    pub request_body_hash: String,
    pub raw_response_hash: String,
    pub output_hash: String,
    pub elapsed_ms: u128,
    pub usage: TokenUsage,
    pub thinking: ThinkingEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAdapterError {
    InvalidRequest(String),
    InvalidConfiguration(String),
    CredentialUnavailable(String),
    CredentialScopeChanged,
    Transport(ProviderTransportError),
    InvalidResponse(String),
    ThinkingEvidenceMissing,
    OutputTooLarge { actual: usize, limit: usize },
}

impl fmt::Display for ProviderAdapterError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(reason) => write!(output, "invalid provider request: {reason}"),
            Self::InvalidConfiguration(reason) => {
                write!(output, "invalid provider configuration: {reason}")
            }
            Self::CredentialUnavailable(name) => {
                write!(output, "provider credential {name} is unavailable")
            }
            Self::CredentialScopeChanged => {
                output.write_str("provider credential scope changed after configuration")
            }
            Self::Transport(error) => write!(output, "provider transport failed: {error}"),
            Self::InvalidResponse(reason) => write!(output, "invalid provider response: {reason}"),
            Self::ThinkingEvidenceMissing => {
                output.write_str("provider response did not contain thinking evidence")
            }
            Self::OutputTooLarge { actual, limit } => {
                write!(
                    output,
                    "provider output is {actual} bytes; limit is {limit}"
                )
            }
        }
    }
}

impl std::error::Error for ProviderAdapterError {}

impl From<ProviderTransportError> for ProviderAdapterError {
    fn from(error: ProviderTransportError) -> Self {
        Self::Transport(error)
    }
}
