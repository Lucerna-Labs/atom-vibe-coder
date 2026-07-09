use crate::graph::Evidence;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    OpenAiResponses,
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
    MissingApiKey { env: String },
    EmptyPrompt,
    Io(String),
    CurlFailed { code: Option<i32>, stderr: String },
    ResponseTextMissing,
}

impl ProviderConfig {
    pub fn from_process_env() -> Self {
        let model =
            std::env::var("MATH_ATOMS_PROVIDER_MODEL").unwrap_or_else(|_| "gpt-5.5".to_string());
        let endpoint = std::env::var("MATH_ATOMS_PROVIDER_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1/responses".to_string());
        let api_key_env = std::env::var("MATH_ATOMS_PROVIDER_KEY_ENV")
            .unwrap_or_else(|_| "OPENAI_API_KEY".to_string());
        let api_key_present = std::env::var(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        Self {
            kind: ProviderKind::OpenAiResponses,
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
                .map(|(_, value)| (*value).to_string())
        };
        let model = lookup("MATH_ATOMS_PROVIDER_MODEL").unwrap_or_else(|| "gpt-5.5".to_string());
        let endpoint = lookup("MATH_ATOMS_PROVIDER_URL")
            .unwrap_or_else(|| "https://api.openai.com/v1/responses".to_string());
        let api_key_env =
            lookup("MATH_ATOMS_PROVIDER_KEY_ENV").unwrap_or_else(|| "OPENAI_API_KEY".to_string());
        let api_key_present = lookup(&api_key_env)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        Self {
            kind: ProviderKind::OpenAiResponses,
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
            body: responses_body(&self.model, &prompt),
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
        fs::write(
            &config_path,
            curl_config(&self.endpoint, &self.model, &api_key, &body_path),
        )?;
        let output = Command::new("curl.exe")
            .arg("--silent")
            .arg("--show-error")
            .arg("--fail-with-body")
            .arg("--config")
            .arg(&config_path)
            .output()
            .or_else(|_| {
                Command::new("curl")
                    .arg("--silent")
                    .arg("--show-error")
                    .arg("--fail-with-body")
                    .arg("--config")
                    .arg(&config_path)
                    .output()
            });
        let cleanup = fs::remove_file(&body_path).and(fs::remove_file(&config_path));
        if let Err(error) = cleanup {
            return Err(ProviderError::Io(format!(
                "provider temp cleanup failed: {error}"
            )));
        }
        let output = output?;
        if !output.status.success() {
            return Err(ProviderError::CurlFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        let body = String::from_utf8_lossy(&output.stdout).to_string();
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
    Err(ProviderError::ResponseTextMissing)
}

fn curl_config(endpoint: &str, model: &str, api_key: &str, body_path: &Path) -> String {
    format!(
        "url = \"{}\"\nrequest = \"POST\"\nheader = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\nheader = \"OpenAI-Beta: responses=v1\"\ndata = \"@{}\"\n# model: {}\n",
        curl_escape(endpoint),
        curl_escape(api_key),
        curl_escape_path(body_path),
        curl_escape(model)
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

fn curl_escape_path(path: &Path) -> String {
    curl_escape(&path.to_string_lossy())
}

fn responses_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"input\":[{{\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}],\"temperature\":0.1}}",
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
}
