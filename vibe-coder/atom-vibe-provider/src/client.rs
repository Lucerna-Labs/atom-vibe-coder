use crate::body::request_body;
use crate::response::{parse_receipt, ReceiptInput};
use crate::{ProviderAdapterError, ProviderRequest, ProviderTurnReceipt};
use math_atoms_core::ProviderConfig;
use math_atoms_hash::sha256_tagged;
use math_atoms_provider_transport::{post_json, ProviderHttpRequest, ProviderTransportError};
use std::time::Instant;

pub trait ProviderCredentialSource {
    fn load(&self, name: &str) -> Result<String, ProviderAdapterError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentCredentials;

impl ProviderCredentialSource for EnvironmentCredentials {
    fn load(&self, name: &str) -> Result<String, ProviderAdapterError> {
        let value = std::env::var(name)
            .map_err(|_| ProviderAdapterError::CredentialUnavailable(name.to_string()))?;
        let value = value.trim().to_string();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ProviderAdapterError::CredentialUnavailable(
                name.to_string(),
            ));
        }
        Ok(value)
    }
}

pub trait ProviderHttp {
    fn post_json(&self, request: ProviderHttpRequest<'_>)
        -> Result<String, ProviderTransportError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CurlProviderHttp;

impl ProviderHttp for CurlProviderHttp {
    fn post_json(
        &self,
        request: ProviderHttpRequest<'_>,
    ) -> Result<String, ProviderTransportError> {
        post_json(request)
    }
}

#[derive(Clone, Debug)]
pub struct ProviderAdapter {
    config: ProviderConfig,
}

impl ProviderAdapter {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderAdapterError> {
        validate_config(&config)?;
        Ok(Self { config })
    }

    pub fn from_process_env() -> Result<Self, ProviderAdapterError> {
        Self::new(ProviderConfig::from_process_env())
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn execute(
        &self,
        request: &ProviderRequest,
    ) -> Result<ProviderTurnReceipt, ProviderAdapterError> {
        self.execute_with(request, &CurlProviderHttp, &EnvironmentCredentials)
    }

    pub fn execute_with(
        &self,
        request: &ProviderRequest,
        http: &dyn ProviderHttp,
        credentials: &dyn ProviderCredentialSource,
    ) -> Result<ProviderTurnReceipt, ProviderAdapterError> {
        request.validate()?;
        validate_config(&self.config)?;
        let api_key = credentials.load(&self.config.api_key_env)?;
        if credential_scope_hash(&self.config.endpoint, &api_key)
            != self.config.credential_scope_hash
        {
            return Err(ProviderAdapterError::CredentialScopeChanged);
        }
        let body = request_body(&self.config, request)?;
        if body.contains(&api_key) {
            return Err(ProviderAdapterError::InvalidConfiguration(
                "credential appeared in provider request body".to_string(),
            ));
        }
        let started = Instant::now();
        let raw = http.post_json(ProviderHttpRequest {
            endpoint: &self.config.endpoint,
            auth_header: &self.config.auth_header,
            auth_scheme: &self.config.auth_scheme,
            api_key: &api_key,
            body_json: &body,
            timeout_seconds: self.config.request_timeout_seconds,
        })?;
        parse_receipt(
            &self.config,
            ReceiptInput {
                request_id: &request.request_id,
                request_body: &body,
                raw_response: &raw,
                elapsed_ms: started.elapsed().as_millis(),
                output_limit: request.max_output_bytes,
            },
        )
    }
}

fn validate_config(config: &ProviderConfig) -> Result<(), ProviderAdapterError> {
    if !config.api_key_present {
        return Err(ProviderAdapterError::CredentialUnavailable(
            config.api_key_env.clone(),
        ));
    }
    if config.endpoint.trim().is_empty()
        || !(config.endpoint.starts_with("https://")
            || config.endpoint.starts_with("http://localhost")
            || config.endpoint.starts_with("http://127.0.0.1")
            || config.endpoint.starts_with("http://[::1]"))
    {
        return Err(ProviderAdapterError::InvalidConfiguration(
            "endpoint must use HTTPS or an explicit loopback HTTP address".to_string(),
        ));
    }
    if config.model.trim().is_empty()
        || config.api_key_env.trim().is_empty()
        || config.auth_header.trim().is_empty()
    {
        return Err(ProviderAdapterError::InvalidConfiguration(
            "model, credential environment name, and auth header are required".to_string(),
        ));
    }
    if config.thinking_level.is_none() {
        return Err(ProviderAdapterError::InvalidConfiguration(
            "thinking must be enabled".to_string(),
        ));
    }
    if config.request_timeout_seconds < 10 || config.request_timeout_seconds > 1_800 {
        return Err(ProviderAdapterError::InvalidConfiguration(
            "request timeout must be between 10 and 1800 seconds".to_string(),
        ));
    }
    Ok(())
}

fn credential_scope_hash(endpoint: &str, api_key: &str) -> String {
    sha256_tagged(format!("{}\0{}", endpoint.trim(), api_key.trim()).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FixedCredential(String);

    impl ProviderCredentialSource for FixedCredential {
        fn load(&self, _name: &str) -> Result<String, ProviderAdapterError> {
            Ok(self.0.clone())
        }
    }

    struct FixedHttp {
        response: String,
        seen_body: Mutex<String>,
        seen_key: Mutex<String>,
    }

    impl ProviderHttp for FixedHttp {
        fn post_json(
            &self,
            request: ProviderHttpRequest<'_>,
        ) -> Result<String, ProviderTransportError> {
            *self.seen_body.lock().unwrap() = request.body_json.to_string();
            *self.seen_key.lock().unwrap() = request.api_key.to_string();
            Ok(self.response.clone())
        }
    }

    fn adapter(key: &str) -> ProviderAdapter {
        ProviderAdapter::new(ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "deepseek"),
            ("MATH_ATOMS_PROVIDER_FORMAT", "chat"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/chat"),
            ("MATH_ATOMS_PROVIDER_MODEL", "deepseek-flash"),
            ("MATH_ATOMS_PROVIDER_THINKING_LEVEL", "low"),
            ("DEEPSEEK_API_KEY", key),
        ]))
        .unwrap()
    }

    #[test]
    fn direct_turn_requires_thinking_and_keeps_key_out_of_body_and_receipt() {
        let key = "unit-test-secret-credential";
        let http = FixedHttp {
            response: r#"{"choices":[{"message":{"reasoning_content":"reasoned","content":"{\"ok\":true}"}}],"usage":{"prompt_tokens":7,"completion_tokens":5,"completion_tokens_details":{"reasoning_tokens":3}}}"#.to_string(),
            seen_body: Mutex::new(String::new()),
            seen_key: Mutex::new(String::new()),
        };
        let receipt = adapter(key)
            .execute_with(
                &ProviderRequest::new("build:step:1", "trusted", "untrusted data"),
                &http,
                &FixedCredential(key.to_string()),
            )
            .unwrap();
        assert_eq!(receipt.text, "{\"ok\":true}");
        assert_eq!(receipt.usage.reasoning_tokens, Some(3));
        assert!(!http.seen_body.lock().unwrap().contains(key));
        assert_eq!(&*http.seen_key.lock().unwrap(), key);
        assert!(!format!("{receipt:?}").contains(key));
    }

    #[test]
    fn credential_rotation_fails_closed_before_http() {
        let http = FixedHttp {
            response: String::new(),
            seen_body: Mutex::new(String::new()),
            seen_key: Mutex::new(String::new()),
        };
        let error = adapter("first")
            .execute_with(
                &ProviderRequest::new("turn-2", "trusted", "data"),
                &http,
                &FixedCredential("second".to_string()),
            )
            .unwrap_err();
        assert_eq!(error, ProviderAdapterError::CredentialScopeChanged);
        assert!(http.seen_body.lock().unwrap().is_empty());
    }
}
