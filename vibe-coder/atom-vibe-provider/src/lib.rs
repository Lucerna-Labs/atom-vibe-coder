//! Direct thinking-model turns over the renderer's credential-safe provider transport.

mod body;
mod client;
mod model;
mod response;

pub use client::{
    CurlProviderHttp, EnvironmentCredentials, ProviderAdapter, ProviderCredentialSource,
    ProviderHttp,
};
pub use model::{
    ProviderAdapterError, ProviderRequest, ProviderTurnReceipt, ThinkingEvidence, TokenUsage,
    MAX_PROVIDER_DATA_BYTES, MAX_PROVIDER_INSTRUCTIONS_BYTES,
};
