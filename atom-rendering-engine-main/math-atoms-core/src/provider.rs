use crate::graph::Evidence;
use std::fs;
use std::io;
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    OpenAiResponses,
    OllamaCloudChat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model: String,
    pub api_key_env: String,
    pub api_key_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedProviderCall {
    pub endpoint: String,
    pub model: String,
    pub api_key_env: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    MissingApiKey {
        env: String,
    },
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
        let model = non_empty_env("MATH_ATOMS_PROVIDER_MODEL")
            .unwrap_or_else(|| default_model(kind).to_string());
        let endpoint = non_empty_env("MATH_ATOMS_PROVIDER_URL")
            .unwrap_or_else(|| default_endpoint(kind).to_string());
        let api_key_env = non_empty_env("MATH_ATOMS_PROVIDER_KEY_ENV")
            .unwrap_or_else(|| default_key_env(kind).to_string());
        let api_key_present = std::env::var(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        Self {
            kind,
            endpoint,
            model,
            api_key_env,
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
        let model =
            lookup("MATH_ATOMS_PROVIDER_MODEL").unwrap_or_else(|| default_model(kind).to_string());
        let endpoint =
            lookup("MATH_ATOMS_PROVIDER_URL").unwrap_or_else(|| default_endpoint(kind).to_string());
        let api_key_env = lookup("MATH_ATOMS_PROVIDER_KEY_ENV")
            .unwrap_or_else(|| default_key_env(kind).to_string());
        let api_key_present = lookup(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        Self {
            kind,
            endpoint,
            model,
            api_key_env,
            api_key_present,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.api_key_present
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
            "Mission: meet or exceed Ornith 1.0 for Math Atoms Coder.\nSelected recipe: {selected_recipe}\nIntent: {intent}\nGraph evidence:\n{context}\nReturn a concise implementation or proof action. Reject unsupported paths."
        );
        Ok(PreparedProviderCall {
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            api_key_env: self.api_key_env.clone(),
            body: provider_body(self.kind, &self.model, &prompt),
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
        let config_path = dir.join(format!("{stem}.curl"));
        fs::write(&body_path, &self.body)?;
        fs::write(&config_path, curl_config(&self.endpoint, &api_key))?;
        let body_arg = format!("@{}", body_path.to_string_lossy());
        let output = Command::new("curl.exe")
            .arg("--silent")
            .arg("--show-error")
            .arg("--fail-with-body")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("45")
            .arg("--write-out")
            .arg("\n__MATH_ATOMS_HTTP_STATUS__:%{http_code}")
            .arg("--config")
            .arg(&config_path)
            .arg("--data-binary")
            .arg(&body_arg)
            .output()
            .or_else(|_| {
                Command::new("curl")
                    .arg("--silent")
                    .arg("--show-error")
                    .arg("--fail-with-body")
                    .arg("--connect-timeout")
                    .arg("10")
                    .arg("--max-time")
                    .arg("45")
                    .arg("--write-out")
                    .arg("\n__MATH_ATOMS_HTTP_STATUS__:%{http_code}")
                    .arg("--config")
                    .arg(&config_path)
                    .arg("--data-binary")
                    .arg(&body_arg)
                    .output()
            });
        let cleanup = fs::remove_file(&body_path).and(fs::remove_file(&config_path));
        if let Err(error) = cleanup {
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
        parse_responses_text(&body)
    }
}

pub fn parse_responses_text(body: &str) -> Result<String, ProviderError> {
    if let Some(pos) = body.find("\"output_text\"") {
        if let Some(text) = read_json_string_after_colon(&body[pos + "\"output_text\"".len()..]) {
            return Ok(text);
        }
    }
    if let Some(pos) = body.find("\"text\"") {
        if let Some(text) = read_json_string_after_colon(&body[pos + "\"text\"".len()..]) {
            return Ok(text);
        }
    }
    if let Some(pos) = body.find("\"response\"") {
        if let Some(text) = read_json_string_after_colon(&body[pos + "\"response\"".len()..]) {
            return Ok(text);
        }
    }
    if let Some(pos) = body.find("\"content\"") {
        if let Some(text) = read_json_string_after_colon(&body[pos + "\"content\"".len()..]) {
            return Ok(text);
        }
    }
    Err(ProviderError::ResponseTextMissing)
}

fn curl_config(endpoint: &str, api_key: &str) -> String {
    format!(
        "url = \"{}\"\nrequest = \"POST\"\nheader = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        curl_escape(endpoint),
        curl_escape(api_key)
    )
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

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
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
        _ => ProviderKind::OpenAiResponses,
    }
}

fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "gpt-5.5",
        ProviderKind::OllamaCloudChat => "gpt-oss:120b",
    }
}

fn default_endpoint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "https://api.openai.com/v1/responses",
        ProviderKind::OllamaCloudChat => "https://ollama.com/api/chat",
    }
}

fn default_key_env(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiResponses => "OPENAI_API_KEY",
        ProviderKind::OllamaCloudChat => "OLLAMA_API_KEY",
    }
}

fn provider_body(kind: ProviderKind, model: &str, prompt: &str) -> String {
    match kind {
        ProviderKind::OpenAiResponses => responses_body(model, prompt),
        ProviderKind::OllamaCloudChat => ollama_chat_body(model, prompt),
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
            node_id: "mission:ornith-parity".to_string(),
            title: "Ornith 1.0 Parity".to_string(),
            excerpt: "Mission evidence".to_string(),
            score: 100,
        }];
        let call = config
            .prepare_call("provider api", "provider-model-loop", &evidence)
            .unwrap();
        assert!(call.body.contains("\"model\":\"gpt-5.5\""));
        assert!(call.body.contains("\"input\""));
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
    fn empty_pair_values_do_not_override_provider_defaults() {
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_MODEL", ""),
            ("MATH_ATOMS_PROVIDER_URL", ""),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", ""),
        ]);
        assert_eq!(config.model, "gpt-5.5");
        assert_eq!(config.endpoint, "https://api.openai.com/v1/responses");
        assert_eq!(config.api_key_env, "OPENAI_API_KEY");
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
}
