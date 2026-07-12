//! Fast single-shot Atom Vibe Coder provider path.
//!
//! Vendored from the flawless v1 build
//! (`C:\Users\jgali\Desktop\v1Atoms Coder by Lucerna Labs\atom-rendering-engine-main\
//! math-atoms-core\src\provider.rs`) with the graph-`Evidence` prompt dropped and a
//! code-generation prompt added. One prompt -> one `curl` call -> extract the fenced
//! code. No 9-packet work plan, no candidate-verification pipeline: this is the fast
//! path the operator wants wired to the Run button. Dependency-free, std-only.
//!
//! Also hosts the native app's vibe-build support so the UI crate stays under its
//! Painted-Fence line cap: `run_fast_build` + `BuildArtifact`, `artifact-window.tsv`
//! manifest parsing (`load_artifacts` / `parse_artifact_manifest`), and the design-upload
//! build gate (`run_design_upload_script` / `design_upload_script_path`).

use std::fs;
use std::io::{self, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Windows `CREATE_NO_WINDOW` process-creation flag: keeps the `curl` subprocess from
/// flashing a black console window over the GUI on every provider call.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey { env } => write!(f, "missing API key in {env}"),
            Self::MissingEndpoint => write!(f, "missing provider endpoint"),
            Self::MissingModel => write!(f, "missing provider model"),
            Self::EmptyPrompt => write!(f, "empty prompt"),
            Self::Io(message) => write!(f, "io error: {message}"),
            Self::CurlFailed {
                code,
                http_status,
                stderr,
                body,
            } => write!(
                f,
                "provider call failed (exit {code:?}, http {http_status:?}): {stderr} {body}"
            ),
            Self::ResponseTextMissing => write!(f, "provider response carried no answer text"),
        }
    }
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

    /// Build a single-shot code-generation call: the model receives the build plan and
    /// returns the COMPLETE implementation in one fenced block. A generous token budget
    /// lets reasoning models finish their hidden reasoning and still emit the code.
    pub fn prepare_build_call(
        &self,
        task: &str,
        plan: &str,
    ) -> Result<PreparedProviderCall, ProviderError> {
        if task.trim().is_empty() {
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
        let prompt = format!(
            "You are the Atom Vibe Coder, a code generator. The atom engine produced this build plan:\n{plan}\n\nTask: {task}\n\nOutput ONLY the complete, compilable, self-contained source code that fulfills the task, inside a single fenced code block (```). No prose before or after. If the language is Rust, produce a complete program with any needed `fn main` and inline `#[test]`s so it builds and runs as-is."
        );
        Ok(PreparedProviderCall {
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            api_key_env: self.api_key_env.clone(),
            auth_header: self.auth_header.clone(),
            auth_scheme: self.auth_scheme.clone(),
            response_key: self.response_key.clone(),
            body: code_provider_body(self.wire_format, &self.model, &prompt, &self.body_template),
        })
    }
}

/// Extract the contents of the first fenced code block from model output. If no fence is
/// present, returns `None` (the caller can fall back to the raw text).
pub fn extract_fenced_code(text: &str) -> Option<String> {
    let open = text.find("```")?;
    let after_fence = &text[open + 3..];
    // The fence line may carry a language tag (```rust); skip to the next line.
    let body_start = after_fence
        .find('\n')
        .map(|i| i + 1)
        .unwrap_or(after_fence.len());
    let rest = &after_fence[body_start..];
    let close = rest.find("```")?;
    Some(rest[..close].trim_end_matches(['\n', '\r']).to_string())
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
        // Best-effort cleanup: the body carries only the prompt, never the API key, so a
        // rare leaked temp file is acceptable; losing a valid answer is not.
        let _ = fs::remove_file(&body_path);
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
    // `reasoning_content` last: some thinking models (e.g. Qwen via LM Studio) leave
    // `content` empty and put the answer — code included — in their reasoning field.
    for key in [
        "output_text",
        "text",
        "response",
        "content",
        "reasoning_content",
    ] {
        if !keys.iter().any(|item| item == key) {
            keys.push(key.to_string());
        }
    }
    for key in keys {
        if let Some(text) = read_json_string_field(body, &key) {
            if !text.trim().is_empty() {
                return Ok(text);
            }
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

/// Whole-turn deadline in seconds. A single call to a quantized local model can need a
/// few minutes; `VIBE_MAX_TIME_SECS` raises the ceiling. Default 300s (one call, not a
/// 9-packet plan), clamped to a sane range.
fn curl_max_time_secs() -> u64 {
    std::env::var("VIBE_MAX_TIME_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&s| (30..=3600).contains(&s))
        .unwrap_or(300)
}

fn curl_args(body_arg: &str) -> Vec<String> {
    [
        "--silent",
        "--show-error",
        "--fail-with-body",
        "--connect-timeout",
        "10",
        "--max-time",
        &curl_max_time_secs().to_string(),
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
    let mut command = Command::new(program);
    command
        .args(curl_args(body_arg))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Suppress the transient black console window on Windows.
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn()?;
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
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
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
        return body.to_string();
    }
    let mut end = MAX;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &body[..end])
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

/// Provider body with a large token budget for code generation.
fn code_provider_body(
    format: ProviderWireFormat,
    model: &str,
    prompt: &str,
    body_template: &str,
) -> String {
    if !body_template.trim().is_empty() {
        return render_body_template(body_template, model, prompt);
    }
    match format {
        ProviderWireFormat::OpenAiResponses => format!(
            "{{\"model\":\"{}\",\"input\":[{{\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}],\"max_output_tokens\":8192}}",
            json_escape(model),
            json_escape(prompt)
        ),
        // ChatCompletions (DeepSeek/Mistral/OpenAI-compatible/LM Studio): omit temperature
        // (reasoning models reject it) and give a large output budget — a thinking model
        // can spend thousands of tokens reasoning before it emits the code.
        ProviderWireFormat::ChatCompletions => format!(
            "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}],\"max_tokens\":16000,\"stream\":false}}",
            json_escape(model),
            json_escape(prompt)
        ),
        ProviderWireFormat::OllamaChat => format!(
            "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}],\"stream\":false}}",
            json_escape(model),
            json_escape(prompt)
        ),
    }
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
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ':') {
        i += 1;
    }
    if i >= chars.len() || chars[i] != '"' {
        return None;
    }
    i += 1;
    let mut out = String::new();
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 1;
            let Some(&esc) = chars.get(i) else { break };
            match esc {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                // \uXXXX — many servers HTML-safe-escape &, <, > (which code is full
                // of). Decode the 4 hex digits, handling surrogate pairs.
                'u' => {
                    if let Some(hex) = chars.get(i + 1..i + 5) {
                        let hex: String = hex.iter().collect();
                        if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                            i += 4;
                            if (0xd800..=0xdbff).contains(&cp) {
                                if chars.get(i + 1) == Some(&'\\') && chars.get(i + 2) == Some(&'u')
                                {
                                    if let Some(low) = chars.get(i + 3..i + 7) {
                                        let low: String = low.iter().collect();
                                        if let Ok(lo) = u32::from_str_radix(&low, 16) {
                                            let scalar =
                                                0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                                            if let Some(c) = char::from_u32(scalar) {
                                                out.push(c);
                                            }
                                            i += 6;
                                        }
                                    }
                                }
                            } else if let Some(c) = char::from_u32(cp) {
                                out.push(c);
                            }
                        }
                    }
                }
                other => out.push(other),
            }
            i += 1;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
            i += 1;
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

/// A built application row shown in the native side-artifacts pane and persisted in the
/// `artifact-window.tsv` manifest. Lives here so provider/build support stays in one
/// crate (and keeps the native UI crate under its Painted-Fence line cap).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildArtifact {
    pub name: String,
    pub status: String,
    pub output: String,
    pub source_path: String,
    pub exe_path: String,
    pub artifact_path: String,
}

/// Result of one fast single-shot build: the artifact row plus the byte count and a
/// short preview the caller shows in the provider-output pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FastBuild {
    pub artifact: BuildArtifact,
    pub bytes: usize,
    pub preview: String,
}

/// Directory where fast-build generated source is written so it shows in the
/// side-artifacts pane: `<cwd>/target/provider-built-apps`, falling back to the temp dir.
pub fn fast_build_dir() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join("target").join("provider-built-apps");
    }
    std::env::temp_dir()
        .join("MathAtomsCoder")
        .join("provider-built-apps")
}

/// Run one fast build: prepare the call, execute the single `curl` request, extract the
/// fenced code, and write it to `out_dir/vibe-build-<stamp>.rs`. `stamp` is supplied by
/// the caller so the file name is deterministic in tests. A write failure is recorded in
/// the artifact's `source_path` rather than failing the build, matching prior behavior.
pub fn run_fast_build(
    config: &ProviderConfig,
    intent: &str,
    plan: &str,
    out_dir: &Path,
    stamp: u128,
) -> Result<FastBuild, String> {
    let call = config
        .prepare_build_call(intent, plan)
        .map_err(|error| error.to_string())?;
    let text = call
        .execute_with_curl()
        .map_err(|error| error.to_string())?;
    let code = extract_fenced_code(&text).unwrap_or(text);
    let name = format!("vibe-build-{stamp}");
    let path = out_dir.join(format!("{name}.rs"));
    let written = match fs::create_dir_all(out_dir).and_then(|_| fs::write(&path, &code)) {
        Ok(()) => path.display().to_string(),
        Err(error) => format!("(not written: {error})"),
    };
    let bytes = code.len();
    let preview: String = code.chars().take(600).collect();
    let output = format!("MATH_ATOMS_APP_OK {name} bytes={bytes}");
    Ok(FastBuild {
        artifact: BuildArtifact {
            name,
            status: "built".to_string(),
            output,
            source_path: written.clone(),
            exe_path: String::new(),
            artifact_path: written,
        },
        bytes,
        preview,
    })
}

/// Load the most recent artifact manifest, returning the built-app rows for the
/// side-artifacts pane. Empty when no manifest is found.
pub fn load_artifacts() -> Vec<BuildArtifact> {
    for path in artifact_manifest_candidates() {
        if let Ok(text) = fs::read_to_string(&path) {
            let artifacts = parse_artifact_manifest(&text);
            if !artifacts.is_empty() {
                return artifacts;
            }
        }
    }
    Vec::new()
}

/// Candidate locations for the `artifact-window.tsv` manifest, most specific first.
pub fn artifact_manifest_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("MATH_ATOMS_ARTIFACT_MANIFEST") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            paths.push(PathBuf::from(trimmed));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("target/provider-built-apps/artifact-window.tsv"));
        paths.push(
            cwd.join("atom-rendering-engine-main/target/provider-built-apps/artifact-window.tsv"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(release_dir) = exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                paths.push(target_dir.join("provider-built-apps/artifact-window.tsv"));
            }
        }
    }
    paths
}

/// Parse a tab-separated artifact manifest (skipping its header row) into artifact rows.
pub fn parse_artifact_manifest(text: &str) -> Vec<BuildArtifact> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 5 || parts[0].trim().is_empty() {
                return None;
            }
            Some(BuildArtifact {
                name: parts[0].trim().to_string(),
                status: parts[1].trim().to_string(),
                output: parts[2].trim().to_string(),
                source_path: parts[3].trim().to_string(),
                exe_path: parts[4].trim().to_string(),
                artifact_path: parts
                    .get(5)
                    .map(|part| part.trim())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Locate the design-upload build gate script (`Test-DesignUploadBuild.ps1`).
pub fn design_upload_script_path() -> Option<PathBuf> {
    let script = "Test-DesignUploadBuild.ps1";
    let mut candidates = Vec::new();
    if let Ok(root) = std::env::var("MATH_ATOMS_SCRIPT_ROOT") {
        candidates.push(PathBuf::from(root).join(script));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("scripts").join(script));
        candidates.push(cwd.join("..").join("scripts").join(script));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(release_dir) = exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                if let Some(engine_dir) = target_dir.parent() {
                    candidates.push(engine_dir.join("..").join("scripts").join(script));
                }
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Run the design-upload build gate (a PowerShell script) with `CREATE_NO_WINDOW` so it
/// never flashes a console window over the GUI.
pub fn run_design_upload_script(
    script: PathBuf,
    html_path: String,
    css_path: String,
) -> Result<String, String> {
    let mut command = Command::new("powershell");
    command
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script);
    if !html_path.trim().is_empty() {
        command.arg("-HtmlPath").arg(html_path.trim());
    }
    if !css_path.trim().is_empty() {
        command.arg("-CssPath").arg(css_path.trim());
    }
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to launch design upload gate: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        if stderr.is_empty() {
            Ok(stdout)
        } else {
            Ok(format!("{stdout}\n{stderr}"))
        }
    } else {
        Err(format!(
            "design upload gate exited {}. stdout: {} stderr: {}",
            output.status, stdout, stderr
        ))
    }
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
    fn build_call_requests_complete_fenced_code_with_large_budget() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_FORMAT", "chat"),
            ("MATH_ATOMS_PROVIDER_MODEL", "qwen"),
            (
                "MATH_ATOMS_PROVIDER_URL",
                "http://127.0.0.1:1234/v1/chat/completions",
            ),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", "VIBE_KEY"),
            ("VIBE_KEY", "secret"),
        ]);
        let call = config
            .prepare_build_call("build a json parser", "plan")
            .unwrap();
        assert!(call.body.contains("\"model\":\"qwen\""));
        assert!(call.body.contains("\"max_tokens\":16000"));
        assert!(call.body.contains("single fenced code block"));
        assert!(!call.body.contains("secret"));
    }

    #[test]
    fn build_call_fails_closed_without_key() {
        let config = ProviderConfig::from_pairs(&[("MATH_ATOMS_PROVIDER_KIND", "openai")]);
        assert_eq!(
            config.prepare_build_call("task", "plan"),
            Err(ProviderError::MissingApiKey {
                env: "OPENAI_API_KEY".to_string()
            })
        );
    }

    #[test]
    fn extract_fenced_code_pulls_the_first_block() {
        let text = "Here you go:\n```rust\nfn main() {}\n```\ntrailing";
        assert_eq!(extract_fenced_code(text).as_deref(), Some("fn main() {}"));
        assert_eq!(extract_fenced_code("no fence here"), None);
    }

    #[test]
    fn response_parser_reads_content_and_reasoning_fallback() {
        assert_eq!(
            parse_responses_text(r#"{"output_text":"ok"}"#).unwrap(),
            "ok"
        );
        // Thinking models may leave content empty and put the code in reasoning_content.
        assert_eq!(
            parse_responses_text(r#"{"content":"","reasoning_content":"pub fn add(){}"}"#).unwrap(),
            "pub fn add(){}"
        );
    }

    #[test]
    fn response_parser_decodes_unicode_escapes() {
        let body = r#"{"content":"fn f(d: &[u8]) { if x << 1 > 0 & y {} }"}"#;
        assert_eq!(
            parse_responses_text(body).unwrap(),
            "fn f(d: &[u8]) { if x << 1 > 0 & y {} }"
        );
    }

    #[test]
    fn http_status_marker_is_split_from_body() {
        let (body, status) =
            split_http_status("{\"error\":\"quota\"}\n__MATH_ATOMS_HTTP_STATUS__:429");
        assert_eq!(body, "{\"error\":\"quota\"}");
        assert_eq!(status, Some(429));
    }

    #[test]
    fn curl_config_carries_secret_via_stdin_not_args() {
        let args = curl_args("@payload.json");
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--config" && pair[1] == "-"));
        assert!(!args.iter().any(|arg| arg.contains("sk-secret")));
        let config = curl_config("https://example.invalid", "x-api-key", "raw", "sk-secret");
        assert!(config.contains("header = \"x-api-key: sk-secret\""));
    }

    #[test]
    fn provider_error_body_redacts_key_material() {
        let body = "Incorrect API key provided: aa0bfd1f****gc8-. You can find your API key.";
        assert_eq!(
            redact_provider_body(body, "real-secret"),
            "Incorrect API key provided: [redacted]. You can find your API key."
        );
    }
}
