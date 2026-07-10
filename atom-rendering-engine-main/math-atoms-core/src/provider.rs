use math_atoms_graph::Evidence;
use math_atoms_hash::sha256_tagged;
use math_atoms_json::{parse as parse_json, JsonValue};
pub use math_atoms_provider_transport::{
    default_provider_output_dir, persist_provider_output, provider_output_hash,
    PersistedProviderOutput,
};
use math_atoms_provider_transport::{
    post_json, ProviderHttpRequest, ProviderTransportError, MAX_PROVIDER_OUTPUT_BYTES,
    MAX_PROVIDER_RESPONSE_BYTES,
};
use math_atoms_work::{
    validate_secure_packet_output, CompletedPacket, WorkError, WorkPlan, WorkPlanStore, WorkStage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    OpenAiResponses,
    OllamaCloudChat,
    MistralChat,
    DeepSeekChat,
    Custom,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai",
            Self::OllamaCloudChat => "ollama",
            Self::MistralChat => "mistral",
            Self::DeepSeekChat => "deepseek",
            Self::Custom => "custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderWireFormat {
    OpenAiResponses,
    ChatCompletions,
    OllamaChat,
}

impl ProviderWireFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "responses",
            Self::ChatCompletions => "chat",
            Self::OllamaChat => "ollama-chat",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub wire_format: ProviderWireFormat,
    pub endpoint: String,
    pub model: String,
    pub api_key_env: String,
    pub auth_header: String,
    pub auth_scheme: String,
    pub body_template: String,
    pub response_key: String,
    pub api_key_present: bool,
    pub credential_scope_hash: String,
    pub request_timeout_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderConfigInput<'a> {
    pub kind_raw: &'a str,
    pub format_raw: &'a str,
    pub model: &'a str,
    pub endpoint: &'a str,
    pub api_key_env: &'a str,
    pub auth_header: &'a str,
    pub auth_scheme: &'a str,
    pub body_template: &'a str,
    pub response_key: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedProviderCall {
    pub endpoint: String,
    pub model: String,
    pub api_key_env: String,
    pub auth_header: String,
    pub auth_scheme: String,
    pub wire_format: ProviderWireFormat,
    pub response_key: String,
    pub body: String,
    pub work_plan: Option<WorkPlan>,
    pub evidence_context: String,
    pub body_template: String,
    pub request_timeout_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    MissingApiKey {
        env: String,
    },
    MissingEndpoint,
    MissingModel,
    EmptyPrompt,
    InvalidBodyTemplate,
    Io(String),
    CurlFailed {
        code: Option<i32>,
        http_status: Option<u16>,
        stderr: String,
        body: String,
    },
    ResponseTextMissing,
    ResponseEnvelopeInvalid,
    ResponseTooLarge,
    WorkPacketFailed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderExecutionOutput {
    pub text: String,
    pub work_plan_id: String,
    pub work_plan_manifest: String,
    pub packet_ids: Vec<String>,
    pub executed_packets: usize,
    pub resumed_packets: usize,
}

impl ProviderConfig {
    pub fn from_process_env() -> Self {
        let kind_raw =
            non_empty_env("MATH_ATOMS_PROVIDER_KIND").unwrap_or_else(|| "openai".to_string());
        let kind = provider_kind_from(&kind_raw);
        let wire_format = non_empty_env("MATH_ATOMS_PROVIDER_FORMAT")
            .map(|value| provider_wire_format_from(&value))
            .unwrap_or_else(|| default_wire_format(kind));
        let model = non_empty_env("MATH_ATOMS_PROVIDER_MODEL")
            .unwrap_or_else(|| default_model(kind).to_string());
        let endpoint = non_empty_env("MATH_ATOMS_PROVIDER_URL")
            .unwrap_or_else(|| default_endpoint(kind).to_string());
        let api_key_env = non_empty_env("MATH_ATOMS_PROVIDER_KEY_ENV")
            .unwrap_or_else(|| default_key_env(kind).to_string());
        let auth_header = non_empty_env("MATH_ATOMS_PROVIDER_AUTH_HEADER")
            .unwrap_or_else(|| default_auth_header().to_string());
        let auth_scheme = std::env::var("MATH_ATOMS_PROVIDER_AUTH_SCHEME")
            .ok()
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| default_auth_scheme().to_string());
        let body_template = non_empty_env("MATH_ATOMS_PROVIDER_BODY_TEMPLATE").unwrap_or_default();
        let response_key = non_empty_env("MATH_ATOMS_PROVIDER_RESPONSE_KEY")
            .unwrap_or_else(|| default_response_key().to_string());
        let api_key = std::env::var(&api_key_env).unwrap_or_default();
        let api_key_present = !api_key.trim().is_empty();
        let credential_scope_hash = credential_scope_hash(&endpoint, &api_key);
        let request_timeout_seconds = provider_timeout_seconds(
            kind,
            &model,
            non_empty_env("MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS").as_deref(),
        );
        Self {
            kind,
            wire_format,
            endpoint,
            model,
            api_key_env,
            auth_header: normalize_header_name(&auth_header),
            auth_scheme: normalize_auth_scheme(&auth_scheme),
            body_template,
            response_key: normalize_response_key(&response_key),
            api_key_present,
            credential_scope_hash,
            request_timeout_seconds,
        }
    }

    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let lookup = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .filter(|(_, value)| !value.trim().is_empty())
                .map(|(_, value)| (*value).to_string())
        };
        let kind = provider_kind_from(
            lookup("MATH_ATOMS_PROVIDER_KIND")
                .unwrap_or_else(|| "openai".to_string())
                .as_str(),
        );
        let wire_format = lookup("MATH_ATOMS_PROVIDER_FORMAT")
            .map(|value| provider_wire_format_from(&value))
            .unwrap_or_else(|| default_wire_format(kind));
        let model =
            lookup("MATH_ATOMS_PROVIDER_MODEL").unwrap_or_else(|| default_model(kind).to_string());
        let endpoint =
            lookup("MATH_ATOMS_PROVIDER_URL").unwrap_or_else(|| default_endpoint(kind).to_string());
        let api_key_env = lookup("MATH_ATOMS_PROVIDER_KEY_ENV")
            .unwrap_or_else(|| default_key_env(kind).to_string());
        let auth_header = lookup("MATH_ATOMS_PROVIDER_AUTH_HEADER")
            .unwrap_or_else(|| default_auth_header().to_string());
        let auth_scheme = pairs
            .iter()
            .find(|(name, _)| *name == "MATH_ATOMS_PROVIDER_AUTH_SCHEME")
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_else(|| default_auth_scheme().to_string());
        let body_template = lookup("MATH_ATOMS_PROVIDER_BODY_TEMPLATE").unwrap_or_default();
        let response_key = lookup("MATH_ATOMS_PROVIDER_RESPONSE_KEY")
            .unwrap_or_else(|| default_response_key().to_string());
        let api_key = lookup(&api_key_env).unwrap_or_default();
        let api_key_present = !api_key.trim().is_empty();
        let credential_scope_hash = credential_scope_hash(&endpoint, &api_key);
        let request_timeout_seconds = provider_timeout_seconds(
            kind,
            &model,
            lookup("MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS").as_deref(),
        );
        Self {
            kind,
            wire_format,
            endpoint,
            model,
            api_key_env,
            auth_header: normalize_header_name(&auth_header),
            auth_scheme: normalize_auth_scheme(&auth_scheme),
            body_template,
            response_key: normalize_response_key(&response_key),
            api_key_present,
            credential_scope_hash,
            request_timeout_seconds,
        }
    }

    pub fn from_values(kind_raw: &str, model: &str, endpoint: &str, api_key_env: &str) -> Self {
        Self::from_values_full(ProviderConfigInput {
            kind_raw,
            format_raw: "",
            model,
            endpoint,
            api_key_env,
            auth_header: "",
            auth_scheme: "",
            body_template: "",
            response_key: "",
        })
    }

    pub fn from_values_full(input: ProviderConfigInput<'_>) -> Self {
        let kind = provider_kind_from(input.kind_raw);
        let wire_format = non_empty_value(input.format_raw)
            .map(|value| provider_wire_format_from(&value))
            .unwrap_or_else(|| default_wire_format(kind));
        let model = non_empty_value(input.model).unwrap_or_else(|| default_model(kind).to_string());
        let endpoint =
            non_empty_value(input.endpoint).unwrap_or_else(|| default_endpoint(kind).to_string());
        let api_key_env =
            non_empty_value(input.api_key_env).unwrap_or_else(|| default_key_env(kind).to_string());
        let auth_header =
            non_empty_value(input.auth_header).unwrap_or_else(|| default_auth_header().to_string());
        let auth_scheme = input.auth_scheme.trim().to_string();
        let auth_scheme = if auth_scheme.is_empty() {
            default_auth_scheme().to_string()
        } else {
            normalize_auth_scheme(&auth_scheme)
        };
        let body_template = input.body_template.trim().to_string();
        let response_key = non_empty_value(input.response_key)
            .unwrap_or_else(|| default_response_key().to_string());
        let api_key = std::env::var(&api_key_env).unwrap_or_default();
        let api_key_present = !api_key.trim().is_empty();
        let credential_scope_hash = credential_scope_hash(&endpoint, &api_key);
        let request_timeout_seconds = provider_timeout_seconds(
            kind,
            &model,
            non_empty_env("MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS").as_deref(),
        );
        Self {
            kind,
            wire_format,
            endpoint,
            model,
            api_key_env,
            auth_header: normalize_header_name(&auth_header),
            auth_scheme,
            body_template,
            response_key: normalize_response_key(&response_key),
            api_key_present,
            credential_scope_hash,
            request_timeout_seconds,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.api_key_present
            && !self.endpoint.trim().is_empty()
            && !self.model.trim().is_empty()
            && !self.auth_header.trim().is_empty()
    }

    pub fn prepare_call(
        &self,
        intent: &str,
        selected_recipe: &str,
        evidence: &[Evidence],
    ) -> Result<PreparedProviderCall, ProviderError> {
        self.prepare_call_with_atoms(intent, selected_recipe, &[], evidence)
    }

    pub fn prepare_call_with_atoms(
        &self,
        intent: &str,
        selected_recipe: &str,
        atom_stack: &[String],
        evidence: &[Evidence],
    ) -> Result<PreparedProviderCall, ProviderError> {
        if intent.trim().is_empty() {
            return Err(ProviderError::EmptyPrompt);
        }
        if self.endpoint.trim().is_empty() {
            return Err(ProviderError::MissingEndpoint);
        }
        if self.model.trim().is_empty() {
            return Err(ProviderError::MissingModel);
        }
        if !self.api_key_present {
            return Err(ProviderError::MissingApiKey {
                env: self.api_key_env.clone(),
            });
        }
        if !self.body_template.trim().is_empty()
            && !self.body_template.contains("{{prompt}}")
            && !self.body_template.contains("{{prompt_json}}")
        {
            return Err(ProviderError::InvalidBodyTemplate);
        }
        let mut context = String::new();
        for item in evidence.iter().take(6) {
            context.push_str("- ");
            context.push_str(&item.title);
            context.push_str(": ");
            context.push_str(&item.excerpt);
            context.push('\n');
        }
        let fingerprint = work_fingerprint(self, evidence);
        let plan = WorkPlan::meticulous(intent, selected_recipe, atom_stack, &fingerprint)
            .map_err(work_error)?;
        let prompt = plan
            .prompt(&plan.packets[0], &[], &context)
            .map_err(work_error)?;
        let work_plan = Some(plan);
        Ok(PreparedProviderCall {
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            api_key_env: self.api_key_env.clone(),
            auth_header: self.auth_header.clone(),
            auth_scheme: self.auth_scheme.clone(),
            wire_format: self.wire_format,
            response_key: self.response_key.clone(),
            body: provider_body(self.wire_format, &self.model, &prompt, &self.body_template),
            work_plan,
            evidence_context: context,
            body_template: self.body_template.clone(),
            request_timeout_seconds: self.request_timeout_seconds,
        })
    }
}

impl PreparedProviderCall {
    pub fn execute_with_curl(&self) -> Result<String, ProviderError> {
        self.execute_with_curl_report().map(|output| output.text)
    }

    pub fn execute_with_curl_report(&self) -> Result<ProviderExecutionOutput, ProviderError> {
        if let Some(plan) = &self.work_plan {
            return self.execute_work_plan(plan.clone());
        }
        self.execute_body_with_curl(&self.body)
            .map(|text| ProviderExecutionOutput {
                text,
                work_plan_id: String::new(),
                work_plan_manifest: String::new(),
                packet_ids: Vec::new(),
                executed_packets: 1,
                resumed_packets: 0,
            })
    }

    fn execute_work_plan(
        &self,
        mut plan: WorkPlan,
    ) -> Result<ProviderExecutionOutput, ProviderError> {
        let store = WorkPlanStore::default();
        let _lease = store.acquire(&plan.id).map_err(work_error)?;
        let mut manifest_path = store.write_plan_manifest(&plan).map_err(work_error)?;
        let mut completed = Vec::new();
        let mut index = 0;
        let mut executed_packets = 0;
        let mut resumed_packets = 0;
        while index < plan.packets.len() {
            let packet = plan.packets[index].clone();
            let validated = if let Some(stored) = store
                .load_packet(&plan, &packet, &self.model)
                .map_err(work_error)?
            {
                resumed_packets += 1;
                validate_secure_packet_output(&packet, &stored.output).map_err(work_error)?
            } else {
                let prompt = plan
                    .prompt(&packet, &completed, &self.evidence_context)
                    .map_err(work_error)?;
                let body =
                    provider_body(self.wire_format, &self.model, &prompt, &self.body_template);
                let raw = self.execute_body_with_curl(&body).map_err(|error| {
                    ProviderError::WorkPacketFailed(format!(
                        "plan {} packet {} provider call failed: {error:?}",
                        plan.id, packet.id
                    ))
                })?;
                let validated = validate_secure_packet_output(&packet, &raw).map_err(work_error)?;
                if packet.stage == WorkStage::FileManifest {
                    plan.expand_files(validated.files.clone())
                        .map_err(work_error)?;
                    manifest_path = store.write_plan_manifest(&plan).map_err(work_error)?;
                }
                store
                    .store_packet(&plan, &packet, &validated.context, &self.model)
                    .map_err(work_error)?;
                executed_packets += 1;
                validated
            };
            if packet.stage == WorkStage::FileManifest && !plan.is_expanded() {
                plan.expand_files(validated.files.clone())
                    .map_err(work_error)?;
                manifest_path = store.write_plan_manifest(&plan).map_err(work_error)?;
            }
            completed.push(CompletedPacket {
                packet_id: packet.id,
                output: validated.context,
            });
            index += 1;
        }
        let text = plan.deliverable(&completed).map_err(work_error)?;
        Ok(ProviderExecutionOutput {
            text,
            work_plan_id: plan.id.clone(),
            work_plan_manifest: manifest_path.to_string_lossy().to_string(),
            packet_ids: plan
                .packets
                .iter()
                .map(|packet| packet.id.clone())
                .collect(),
            executed_packets,
            resumed_packets,
        })
    }

    fn execute_body_with_curl(&self, body_json: &str) -> Result<String, ProviderError> {
        let api_key = std::env::var(&self.api_key_env)
            .map_err(|_| ProviderError::MissingApiKey {
                env: self.api_key_env.clone(),
            })?
            .trim()
            .to_string();
        if api_key.is_empty() {
            return Err(ProviderError::MissingApiKey {
                env: self.api_key_env.clone(),
            });
        }
        let body = post_json(ProviderHttpRequest {
            endpoint: &self.endpoint,
            auth_header: &self.auth_header,
            auth_scheme: &self.auth_scheme,
            api_key: &api_key,
            body_json,
            timeout_seconds: self.request_timeout_seconds,
        })
        .map_err(provider_transport_error)?;
        parse_provider_text(&body, self.wire_format, &self.response_key)
    }
}

pub fn parse_responses_text(body: &str) -> Result<String, ProviderError> {
    parse_provider_text(
        body,
        ProviderWireFormat::OpenAiResponses,
        default_response_key(),
    )
}

fn parse_provider_text(
    body: &str,
    wire_format: ProviderWireFormat,
    preferred_key: &str,
) -> Result<String, ProviderError> {
    if body.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(ProviderError::ResponseTooLarge);
    }
    let root = parse_json(body).map_err(|_| ProviderError::ResponseEnvelopeInvalid)?;
    let Some(object) = root.as_object() else {
        return Err(ProviderError::ResponseEnvelopeInvalid);
    };
    if root
        .get("error")
        .is_some_and(|error| !matches!(error, JsonValue::Null))
    {
        return Err(ProviderError::ResponseEnvelopeInvalid);
    }
    let preferred = normalize_response_key(preferred_key);
    if !preferred.is_empty() && preferred != default_response_key() {
        if let Some(text) = root.get(&preferred).and_then(JsonValue::as_str) {
            return validated_provider_text(text);
        }
    }
    let text = match wire_format {
        ProviderWireFormat::OpenAiResponses => responses_output_text(&root),
        ProviderWireFormat::ChatCompletions => chat_completion_text(&root),
        ProviderWireFormat::OllamaChat => ollama_chat_text(&root),
    };
    if object.is_empty() {
        return Err(ProviderError::ResponseEnvelopeInvalid);
    }
    validated_provider_text(text.ok_or(ProviderError::ResponseTextMissing)?)
}

fn responses_output_text(root: &JsonValue) -> Option<&str> {
    if let Some(text) = root.get("output_text").and_then(JsonValue::as_str) {
        return Some(text);
    }
    for item in root.get("output")?.as_array()? {
        for content in item.get("content")?.as_array()? {
            let kind = content.get("type").and_then(JsonValue::as_str);
            if matches!(kind, Some("output_text" | "text")) {
                if let Some(text) = content.get("text").and_then(JsonValue::as_str) {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn chat_completion_text(root: &JsonValue) -> Option<&str> {
    root.get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
}

fn ollama_chat_text(root: &JsonValue) -> Option<&str> {
    root.get("message")?.get("content")?.as_str()
}

fn validated_provider_text(text: &str) -> Result<String, ProviderError> {
    if text.trim().is_empty() {
        return Err(ProviderError::ResponseTextMissing);
    }
    if text.len() > MAX_PROVIDER_OUTPUT_BYTES {
        return Err(ProviderError::ResponseTooLarge);
    }
    Ok(text.to_string())
}

fn normalize_header_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect();
    if cleaned.is_empty() {
        default_auth_header().to_string()
    } else {
        cleaned
    }
}

fn normalize_auth_scheme(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "raw" | "none" | "no-prefix" | "no_prefix" => String::new(),
        _ => value.trim().to_string(),
    }
}

fn normalize_response_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect()
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn non_empty_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn work_fingerprint(config: &ProviderConfig, evidence: &[Evidence]) -> String {
    let mut value = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        config.kind.as_str(),
        config.model,
        config.endpoint,
        config.wire_format.as_str(),
        config.auth_header,
        config.auth_scheme,
        config.response_key,
        sha256_tagged(config.body_template.as_bytes()),
        config.credential_scope_hash,
        config.request_timeout_seconds
    );
    for item in evidence.iter().take(8) {
        value.push('\0');
        value.push_str(&item.node_id);
        value.push(':');
        value.push_str(&item.score.to_string());
        value.push(':');
        value.push_str(&sha256_tagged(item.excerpt.as_bytes()));
    }
    sha256_tagged(value.as_bytes())
}

fn credential_scope_hash(endpoint: &str, api_key: &str) -> String {
    if api_key.trim().is_empty() {
        return String::new();
    }
    sha256_tagged(format!("{}\0{}", endpoint.trim(), api_key.trim()).as_bytes())
}

fn provider_timeout_seconds(kind: ProviderKind, model: &str, configured: Option<&str>) -> u64 {
    let fallback =
        if kind == ProviderKind::DeepSeekChat && model.to_ascii_lowercase().contains("pro") {
            900
        } else {
            120
        };
    configured
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| (10..=1_800).contains(value))
        .unwrap_or(fallback)
}

fn provider_transport_error(error: ProviderTransportError) -> ProviderError {
    match error {
        ProviderTransportError::Io(reason) => ProviderError::Io(reason),
        ProviderTransportError::CurlFailed {
            code,
            http_status,
            stderr,
            body,
        } => ProviderError::CurlFailed {
            code,
            http_status,
            stderr,
            body,
        },
        ProviderTransportError::ResponseTooLarge => ProviderError::ResponseTooLarge,
    }
}

fn work_error(error: WorkError) -> ProviderError {
    ProviderError::WorkPacketFailed(error.to_string())
}

fn provider_kind_from(value: &str) -> ProviderKind {
    match value.to_ascii_lowercase().as_str() {
        "ollama" | "ollama-cloud" | "ollama_cloud" => ProviderKind::OllamaCloudChat,
        "mistral" | "mistral-ai" | "mistral_ai" | "vibe" | "mistral-vibe" => {
            ProviderKind::MistralChat
        }
        "deepseek" | "deepseek-pro" | "deepseek_pro" | "deepseek-v4-pro" | "deepseek-flash"
        | "deepseek_flash" | "deepseek-v4-flash" => ProviderKind::DeepSeekChat,
        "custom" | "generic" | "compatible" | "openai-compatible" | "openai_chat"
        | "openai-chat" => ProviderKind::Custom,
        _ => ProviderKind::OpenAiResponses,
    }
}

fn provider_wire_format_from(value: &str) -> ProviderWireFormat {
    match value.to_ascii_lowercase().as_str() {
        "ollama" | "ollama-chat" | "ollama_chat" => ProviderWireFormat::OllamaChat,
        "chat" | "chat-completions" | "chat_completions" | "openai-chat" | "mistral" => {
            ProviderWireFormat::ChatCompletions
        }
        _ => ProviderWireFormat::OpenAiResponses,
    }
}

fn default_wire_format(kind: ProviderKind) -> ProviderWireFormat {
    match kind {
        ProviderKind::OpenAiResponses => ProviderWireFormat::OpenAiResponses,
        ProviderKind::OllamaCloudChat => ProviderWireFormat::OllamaChat,
        ProviderKind::MistralChat | ProviderKind::DeepSeekChat | ProviderKind::Custom => {
            ProviderWireFormat::ChatCompletions
        }
    }
}

fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "gpt-5.5",
        ProviderKind::OllamaCloudChat => "gpt-oss:120b",
        ProviderKind::MistralChat => "mistral-large-latest",
        ProviderKind::DeepSeekChat => "deepseek-v4-pro",
        ProviderKind::Custom => "",
    }
}

fn default_endpoint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "https://api.openai.com/v1/responses",
        ProviderKind::OllamaCloudChat => "https://ollama.com/api/chat",
        ProviderKind::MistralChat => "https://api.mistral.ai/v1/chat/completions",
        ProviderKind::DeepSeekChat => "https://api.deepseek.com/chat/completions",
        ProviderKind::Custom => "",
    }
}

fn default_key_env(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "OPENAI_API_KEY",
        ProviderKind::OllamaCloudChat => "OLLAMA_API_KEY",
        ProviderKind::MistralChat => "MISTRAL_API_KEY",
        ProviderKind::DeepSeekChat => "DEEPSEEK_API_KEY",
        ProviderKind::Custom => "MATH_ATOMS_PROVIDER_API_KEY",
    }
}

fn default_auth_header() -> &'static str {
    "Authorization"
}

fn default_auth_scheme() -> &'static str {
    "Bearer"
}

fn default_response_key() -> &'static str {
    "output_text"
}

fn provider_body(
    format: ProviderWireFormat,
    model: &str,
    prompt: &str,
    body_template: &str,
) -> String {
    if !body_template.trim().is_empty() {
        return render_body_template(body_template, model, prompt);
    }
    match format {
        ProviderWireFormat::OpenAiResponses => responses_body(model, prompt),
        ProviderWireFormat::ChatCompletions if model.eq_ignore_ascii_case("deepseek-v4-pro") => {
            deepseek_pro_body(model, prompt)
        }
        ProviderWireFormat::ChatCompletions => chat_completions_body(model, prompt),
        ProviderWireFormat::OllamaChat => ollama_chat_body(model, prompt),
    }
}

fn responses_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"input\":[{{\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}],\"temperature\":0.1}}",
        json_escape(model),
        json_escape(prompt)
    )
}

fn ollama_chat_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}],\"stream\":false}}",
        json_escape(model),
        json_escape(prompt)
    )
}

fn chat_completions_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}],\"temperature\":0.1,\"stream\":false}}",
        json_escape(model),
        json_escape(prompt)
    )
}

fn deepseek_pro_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}],\"thinking\":{{\"type\":\"enabled\"}},\"reasoning_effort\":\"max\",\"stream\":false}}",
        json_escape(model),
        json_escape(prompt)
    )
}

fn render_body_template(template: &str, model: &str, prompt: &str) -> String {
    template
        .replace("{{model}}", &json_escape(model))
        .replace("{{prompt}}", &json_escape(prompt))
        .replace("{{model_json}}", &format!("\"{}\"", json_escape(model)))
        .replace("{{prompt_json}}", &format!("\"{}\"", json_escape(prompt)))
}

fn json_escape(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_body_uses_responses_shape_without_secret() {
        let config = ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "secret")]);
        let evidence = vec![Evidence {
            node_id: "mission:production-app-build".to_string(),
            title: "Production App Build".to_string(),
            excerpt: "Mission evidence".to_string(),
            score: 100,
        }];
        let call = config
            .prepare_call("provider api", "provider-model-loop", &evidence)
            .unwrap();
        assert!(call.body.contains("\"model\":\"gpt-5.5\""));
        assert!(call.body.contains("\"input\""));
        assert_eq!(config.wire_format, ProviderWireFormat::OpenAiResponses);
        assert_eq!(call.wire_format, ProviderWireFormat::OpenAiResponses);
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn response_text_parser_reads_output_text() {
        let text = parse_responses_text(r#"{"output_text":"route proven\nnext"}"#).unwrap();
        assert_eq!(text, "route proven\nnext");
        let nested = parse_responses_text(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"nested route"}]}]}"#,
        )
        .unwrap();
        assert_eq!(nested, "nested route");
    }

    #[test]
    fn ollama_provider_uses_cloud_chat_shape() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "ollama"),
            ("OLLAMA_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("provider api", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(config.kind, ProviderKind::OllamaCloudChat);
        assert_eq!(config.wire_format, ProviderWireFormat::OllamaChat);
        assert_eq!(call.endpoint, "https://ollama.com/api/chat");
        assert!(call.body.contains("\"messages\""));
        assert!(call.body.contains("\"stream\":false"));
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn response_text_parser_reads_ollama_content() {
        let text = parse_provider_text(
            r#"{"message":{"role":"assistant","content":"provider-ok"}}"#,
            ProviderWireFormat::OllamaChat,
            "content",
        )
        .unwrap();
        assert_eq!(text, "provider-ok");
    }

    #[test]
    fn response_text_parser_reads_chat_completion_content() {
        let text = parse_provider_text(
            r#"{"choices":[{"message":{"role":"assistant","content":"mistral-ok"}}]}"#,
            ProviderWireFormat::ChatCompletions,
            "content",
        )
        .unwrap();
        assert_eq!(text, "mistral-ok");
    }

    #[test]
    fn response_parser_rejects_wrong_paths_and_error_envelopes() {
        assert_eq!(
            parse_provider_text(
                r#"{"error":{"content":"quota exceeded"}}"#,
                ProviderWireFormat::ChatCompletions,
                "content",
            ),
            Err(ProviderError::ResponseEnvelopeInvalid)
        );
        assert_eq!(
            parse_provider_text(
                r#"{"content":[{"type":"text","text":"wrong-path"}]}"#,
                ProviderWireFormat::OpenAiResponses,
                "output_text",
            ),
            Err(ProviderError::ResponseTextMissing)
        );
        assert_eq!(
            parse_provider_text(
                r#"{"choices":[{"message":{"content":""}}]}"#,
                ProviderWireFormat::ChatCompletions,
                "content",
            ),
            Err(ProviderError::ResponseTextMissing)
        );
    }

    #[test]
    fn mistral_provider_uses_chat_completions_profile() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "mistral"),
            ("MISTRAL_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("provider api", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(config.kind, ProviderKind::MistralChat);
        assert_eq!(config.wire_format, ProviderWireFormat::ChatCompletions);
        assert_eq!(config.model, "mistral-large-latest");
        assert_eq!(call.endpoint, "https://api.mistral.ai/v1/chat/completions");
        assert!(call.body.contains("\"messages\""));
        assert!(call.body.contains("\"role\":\"user\""));
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn deepseek_provider_uses_pro_thinking_profile() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "deepseek"),
            ("DEEPSEEK_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("build a toy app", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(config.kind, ProviderKind::DeepSeekChat);
        assert_eq!(config.wire_format, ProviderWireFormat::ChatCompletions);
        assert_eq!(config.model, "deepseek-v4-pro");
        assert_eq!(call.endpoint, "https://api.deepseek.com/chat/completions");
        assert!(call.body.contains("\"model\":\"deepseek-v4-pro\""));
        assert!(call.body.contains("\"thinking\":{\"type\":\"enabled\"}"));
        assert!(call.body.contains("\"reasoning_effort\":\"max\""));
        assert!(call.body.contains("\"stream\":false"));
        assert!(!call.body.contains("\"temperature\""));
        assert!(!call.body.contains("deepseek-v4-flash"));
        assert!(!call.body.contains("secret"));
        assert_eq!(config.request_timeout_seconds, 900);
        assert_eq!(call.request_timeout_seconds, 900);
    }

    #[test]
    fn custom_provider_accepts_endpoint_format_and_auth_knobs() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_FORMAT", "chat"),
            ("MATH_ATOMS_PROVIDER_MODEL", "vibe-model"),
            (
                "MATH_ATOMS_PROVIDER_URL",
                "https://example.invalid/v1/chat/completions",
            ),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", "VIBE_API_KEY"),
            ("MATH_ATOMS_PROVIDER_AUTH_HEADER", "x-api-key"),
            ("MATH_ATOMS_PROVIDER_AUTH_SCHEME", "raw"),
            ("VIBE_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("provider api", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(config.kind, ProviderKind::Custom);
        assert_eq!(config.wire_format, ProviderWireFormat::ChatCompletions);
        assert_eq!(call.auth_header, "x-api-key");
        assert_eq!(call.auth_scheme, "");
        assert!(call.body.contains("\"model\":\"vibe-model\""));
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn custom_provider_requires_endpoint_model_and_key_before_ready() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "secret"),
        ]);
        assert!(!config.is_ready());
        assert_eq!(
            config.prepare_call("provider api", "provider-model-loop", &[]),
            Err(ProviderError::MissingEndpoint)
        );
    }

    #[test]
    fn custom_provider_template_and_response_key_are_applied() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_MODEL", "vibe-model"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/run"),
            (
                "MATH_ATOMS_PROVIDER_BODY_TEMPLATE",
                "{\"m\":{{model_json}},\"p\":{{prompt_json}}}",
            ),
            ("MATH_ATOMS_PROVIDER_RESPONSE_KEY", "answer"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("template provider", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(call.response_key, "answer");
        assert!(call
            .body
            .starts_with("{\"m\":\"vibe-model\",\"p\":\"Atom Vibe Coder meticulous work packet."));
        assert_eq!(call.work_plan.as_ref().unwrap().packets.len(), 5);
        assert_eq!(
            parse_provider_text(
                r#"{"answer":"template-ok"}"#,
                call.wire_format,
                &call.response_key,
            )
            .unwrap(),
            "template-ok"
        );
    }

    #[test]
    fn custom_template_must_carry_each_work_packet_prompt() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_MODEL", "custom-model"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/run"),
            ("MATH_ATOMS_PROVIDER_BODY_TEMPLATE", "{\"static\":true}"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "secret"),
        ]);
        assert_eq!(
            config.prepare_call("build an app", "provider-model-loop", &[]),
            Err(ProviderError::InvalidBodyTemplate)
        );
    }

    #[test]
    fn work_plan_identity_changes_with_provider_request_contract() {
        let base = [
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_MODEL", "custom-model"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/run"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "secret"),
        ];
        let first = ProviderConfig::from_pairs(&base)
            .prepare_call("build an app", "provider-model-loop", &[])
            .unwrap();
        let mut changed = base.to_vec();
        changed.push((
            "MATH_ATOMS_PROVIDER_BODY_TEMPLATE",
            "{\"model\":{{model_json}},\"request\":{{prompt_json}}}",
        ));
        let second = ProviderConfig::from_pairs(&changed)
            .prepare_call("build an app", "provider-model-loop", &[])
            .unwrap();
        assert_ne!(first.work_plan.unwrap().id, second.work_plan.unwrap().id);
    }

    #[test]
    fn work_plan_resume_identity_is_credential_scoped() {
        let first = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_MODEL", "custom-model"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/run"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "tenant-secret-one"),
        ]);
        let second = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_MODEL", "custom-model"),
            ("MATH_ATOMS_PROVIDER_URL", "https://example.invalid/run"),
            ("MATH_ATOMS_PROVIDER_API_KEY", "tenant-secret-two"),
        ]);
        assert_ne!(first.credential_scope_hash, second.credential_scope_hash);
        let first_plan = first
            .prepare_call("build an app", "provider-model-loop", &[])
            .unwrap()
            .work_plan
            .unwrap();
        let second_plan = second
            .prepare_call("build an app", "provider-model-loop", &[])
            .unwrap()
            .work_plan
            .unwrap();
        assert_ne!(first_plan.id, second_plan.id);
    }

    #[test]
    fn provider_timeout_override_is_bounded() {
        let configured = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "deepseek"),
            ("DEEPSEEK_API_KEY", "secret"),
            ("MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS", "1200"),
        ]);
        assert_eq!(configured.request_timeout_seconds, 1200);
        let invalid = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "deepseek"),
            ("DEEPSEEK_API_KEY", "secret"),
            ("MATH_ATOMS_PROVIDER_TIMEOUT_SECONDS", "999999"),
        ]);
        assert_eq!(invalid.request_timeout_seconds, 900);
    }

    #[test]
    fn empty_pair_values_do_not_override_provider_defaults() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_MODEL", ""),
            ("MATH_ATOMS_PROVIDER_URL", ""),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", ""),
        ]);
        assert_eq!(config.model, "gpt-5.5");
        assert_eq!(config.endpoint, "https://api.openai.com/v1/responses");
        assert_eq!(config.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.auth_header, "Authorization");
        assert_eq!(config.auth_scheme, "Bearer");
        assert_eq!(config.body_template, "");
        assert_eq!(config.response_key, "output_text");
    }

    #[test]
    fn ui_provider_values_apply_defaults_and_key_presence() {
        let key = format!("MATH_ATOMS_UI_TEST_KEY_{}", std::process::id());
        std::env::set_var(&key, "secret");
        let config = ProviderConfig::from_values("ollama-cloud", "", "", &key);
        std::env::remove_var(&key);
        assert_eq!(config.kind, ProviderKind::OllamaCloudChat);
        assert_eq!(config.kind.as_str(), "ollama");
        assert_eq!(config.wire_format.as_str(), "ollama-chat");
        assert_eq!(config.model, "gpt-oss:120b");
        assert_eq!(config.endpoint, "https://ollama.com/api/chat");
        assert_eq!(config.api_key_env, key);
        assert!(config.api_key_present);
    }

    #[test]
    fn provider_output_hash_is_stable_for_audit_records() {
        assert_eq!(
            provider_output_hash("provider proof"),
            provider_output_hash("provider proof")
        );
        assert!(provider_output_hash("provider proof").starts_with("sha256:"));
    }
}
