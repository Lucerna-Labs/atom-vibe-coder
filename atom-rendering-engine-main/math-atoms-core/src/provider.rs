use crate::graph::Evidence;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Output, Stdio};

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
    pub response_key: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    MissingApiKey {
        env: String,
    },
    MissingEndpoint,
    MissingModel,
    EmptyPrompt,
    Io(String),
    CurlFailed {
        code: Option<i32>,
        http_status: Option<u16>,
        stderr: String,
        body: String,
    },
    ResponseTextMissing,
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
        let api_key_present = std::env::var(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
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
        let api_key_present = lookup(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
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
        let api_key_present = std::env::var(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
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
        let mut context = String::new();
        for item in evidence.iter().take(6) {
            context.push_str("- ");
            context.push_str(&item.title);
            context.push_str(": ");
            context.push_str(&item.excerpt);
            context.push('\n');
        }
        let prompt = format!(
            "Mission: build the requested app through Atom Vibe Coder using the selected recipe and current graph evidence.\nSelected recipe: {selected_recipe}\nIntent: {intent}\nGraph evidence:\n{context}\nReturn a concise implementation or proof action for that request. Reject unsupported paths."
        );
        Ok(PreparedProviderCall {
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            api_key_env: self.api_key_env.clone(),
            auth_header: self.auth_header.clone(),
            auth_scheme: self.auth_scheme.clone(),
            response_key: self.response_key.clone(),
            body: provider_body(self.wire_format, &self.model, &prompt, &self.body_template),
        })
    }
}

impl PreparedProviderCall {
    pub fn execute_with_curl(&self) -> Result<String, ProviderError> {
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
        let dir = std::env::temp_dir();
        let stem = format!(
            "math-atoms-provider-{}-{}",
            std::process::id(),
            unique_suffix()
        );
        let body_path = dir.join(format!("{stem}.json"));
        fs::write(&body_path, &self.body)?;
        let body_arg = format!("@{}", body_path.to_string_lossy());
        let config = curl_config(
            &self.endpoint,
            &self.auth_header,
            &self.auth_scheme,
            &api_key,
        );
        let output = run_curl_with_stdin_config("curl.exe", &body_arg, &config)
            .or_else(|_| run_curl_with_stdin_config("curl", &body_arg, &config));
        if let Err(error) = fs::remove_file(&body_path) {
            return Err(ProviderError::Io(format!(
                "provider temp cleanup failed: {error}"
            )));
        }
        let output = output?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let (body, http_status) = split_http_status(&stdout);
        if !output.status.success() {
            return Err(ProviderError::CurlFailed {
                code: output.status.code(),
                http_status,
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                body: truncate_for_log(&redact_provider_body(&body, &api_key)),
            });
        }
        parse_provider_text(&body, &self.response_key)
    }
}

pub fn parse_responses_text(body: &str) -> Result<String, ProviderError> {
    parse_provider_text(body, default_response_key())
}

fn parse_provider_text(body: &str, preferred_key: &str) -> Result<String, ProviderError> {
    let mut keys = Vec::new();
    let preferred = normalize_response_key(preferred_key);
    if !preferred.is_empty() {
        keys.push(preferred);
    }
    for key in ["output_text", "text", "response", "content"] {
        if !keys.iter().any(|item| item == key) {
            keys.push(key.to_string());
        }
    }
    for key in keys {
        if let Some(text) = read_json_string_field(body, &key) {
            return Ok(text);
        }
    }
    Err(ProviderError::ResponseTextMissing)
}

pub fn provider_output_hash(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv:{hash:016x}")
}

fn curl_config(endpoint: &str, auth_header: &str, auth_scheme: &str, api_key: &str) -> String {
    let auth_scheme = normalize_auth_scheme(auth_scheme);
    let auth_value = if auth_scheme.trim().is_empty() {
        api_key.to_string()
    } else {
        format!("{} {}", auth_scheme.trim(), api_key)
    };
    format!(
        "url = \"{}\"\nrequest = \"POST\"\nheader = \"{}: {}\"\nheader = \"Content-Type: application/json\"\n",
        curl_escape(endpoint),
        curl_escape(&normalize_header_name(auth_header)),
        curl_escape(&auth_value)
    )
}

fn curl_args(body_arg: &str) -> Vec<String> {
    [
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--connect-timeout",
        "10",
        "--max-time",
        "45",
        "--write-out",
        "\n__MATH_ATOMS_HTTP_STATUS__:%{http_code}",
        "--config",
        "-",
        "--data-binary",
        body_arg,
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn run_curl_with_stdin_config(program: &str, body_arg: &str, config: &str) -> io::Result<Output> {
    let mut child = Command::new(program)
        .args(curl_args(body_arg))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "curl stdin was not available",
        ));
    };
    stdin.write_all(config.as_bytes())?;
    drop(stdin);
    child.wait_with_output()
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn curl_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
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

fn split_http_status(stdout: &str) -> (String, Option<u16>) {
    let marker = "\n__MATH_ATOMS_HTTP_STATUS__:";
    if let Some(pos) = stdout.rfind(marker) {
        let body = stdout[..pos].to_string();
        let status = stdout[pos + marker.len()..].trim().parse::<u16>().ok();
        return (body, status);
    }
    (stdout.to_string(), None)
}

fn truncate_for_log(body: &str) -> String {
    const MAX: usize = 700;
    if body.len() <= MAX {
        body.to_string()
    } else {
        format!("{}...", &body[..MAX])
    }
}

fn redact_provider_body(body: &str, api_key: &str) -> String {
    let mut text = if api_key.is_empty() {
        body.to_string()
    } else {
        body.replace(api_key, "[redacted]")
    };
    let marker = "Incorrect API key provided: ";
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find(marker) {
        let start = cursor + offset;
        let value_start = start + marker.len();
        let Some(end_offset) = text[value_start..].find(". You can") else {
            break;
        };
        let value_end = value_start + end_offset;
        text.replace_range(value_start..value_end, "[redacted]");
        cursor = value_start + "[redacted]".len();
    }
    text
}

fn provider_kind_from(value: &str) -> ProviderKind {
    match value.to_ascii_lowercase().as_str() {
        "ollama" | "ollama-cloud" | "ollama_cloud" => ProviderKind::OllamaCloudChat,
        "mistral" | "mistral-ai" | "mistral_ai" | "vibe" | "mistral-vibe" => {
            ProviderKind::MistralChat
        }
        "deepseek" | "deepseek-flash" | "deepseek_flash" | "deepseek-v4-flash" => {
            ProviderKind::DeepSeekChat
        }
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
        ProviderKind::DeepSeekChat => "deepseek-v4-flash",
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

fn render_body_template(template: &str, model: &str, prompt: &str) -> String {
    template
        .replace("{{model}}", &json_escape(model))
        .replace("{{prompt}}", &json_escape(prompt))
        .replace("{{model_json}}", &format!("\"{}\"", json_escape(model)))
        .replace("{{prompt_json}}", &format!("\"{}\"", json_escape(prompt)))
}

fn read_json_string_field(input: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut cursor = 0;
    while let Some(offset) = input[cursor..].find(&needle) {
        let start = cursor + offset + needle.len();
        if let Some(text) = read_json_string_after_colon(&input[start..]) {
            return Some(text);
        }
        cursor = start;
    }
    None
}

fn read_json_string_after_colon(input: &str) -> Option<String> {
    let mut chars = input
        .chars()
        .skip_while(|ch| ch.is_whitespace() || *ch == ':');
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => out.push(ch),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
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

impl From<io::Error> for ProviderError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
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
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn response_text_parser_reads_output_text() {
        let text = parse_responses_text(r#"{"output_text":"route proven\nnext"}"#).unwrap();
        assert_eq!(text, "route proven\nnext");
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
        let text =
            parse_responses_text(r#"{"message":{"role":"assistant","content":"provider-ok"}}"#)
                .unwrap();
        assert_eq!(text, "provider-ok");
    }

    #[test]
    fn response_text_parser_reads_chat_completion_content() {
        let text = parse_responses_text(
            r#"{"choices":[{"message":{"role":"assistant","content":"mistral-ok"}}]}"#,
        )
        .unwrap();
        assert_eq!(text, "mistral-ok");
    }

    #[test]
    fn response_text_parser_skips_non_field_matches() {
        let text =
            parse_responses_text(r#"{"content":[{"type":"text","text":"anthropic-ok"}]}"#).unwrap();
        assert_eq!(text, "anthropic-ok");
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
    fn deepseek_provider_uses_flash_chat_profile() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "deepseek"),
            ("DEEPSEEK_API_KEY", "secret"),
        ]);
        let call = config
            .prepare_call("build a toy app", "provider-model-loop", &[])
            .unwrap();
        assert_eq!(config.kind, ProviderKind::DeepSeekChat);
        assert_eq!(config.wire_format, ProviderWireFormat::ChatCompletions);
        assert_eq!(config.model, "deepseek-v4-flash");
        assert_eq!(call.endpoint, "https://api.deepseek.com/chat/completions");
        assert!(call.body.contains("\"model\":\"deepseek-v4-flash\""));
        assert!(call.body.contains("\"stream\":false"));
        assert!(!call.body.contains("deepseek-v4-pro"));
        assert!(!call.body.contains("secret"));
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
            .starts_with("{\"m\":\"vibe-model\",\"p\":\"Mission:"));
        assert_eq!(
            parse_provider_text(r#"{"answer":"template-ok"}"#, &call.response_key).unwrap(),
            "template-ok"
        );
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
    fn http_status_marker_is_split_from_body() {
        let (body, status) =
            split_http_status("{\"error\":\"quota\"}\n__MATH_ATOMS_HTTP_STATUS__:429");
        assert_eq!(body, "{\"error\":\"quota\"}");
        assert_eq!(status, Some(429));
    }

    #[test]
    fn provider_error_body_redacts_key_material() {
        let body = "Incorrect API key provided: aa0bfd1f********gc8-. You can find your API key.";
        let redacted = redact_provider_body(body, "real-secret");
        assert_eq!(
            redacted,
            "Incorrect API key provided: [redacted]. You can find your API key."
        );
    }

    #[test]
    fn provider_output_hash_is_stable_for_audit_records() {
        assert_eq!(
            provider_output_hash("provider proof"),
            provider_output_hash("provider proof")
        );
        assert!(provider_output_hash("provider proof").starts_with("fnv:"));
    }

    #[test]
    fn curl_command_config_comes_from_stdin_not_temp_file_or_args() {
        let secret = "sk-test-secret";
        let args = curl_args("@payload.json");
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--config" && pair[1] == "-"));
        assert!(!args.iter().any(|arg| arg.contains(secret)));
        assert!(!args.iter().any(|arg| arg.ends_with(".curl")));
        let config = curl_config(
            "https://example.invalid",
            "x-api-key",
            "raw",
            "sk-test-secret",
        );
        assert!(config.contains("header = \"x-api-key: sk-test-secret\""));
    }
}
