//! Bounded credential-safe provider HTTP transport and content-addressed output evidence.

use math_atoms_hash::{sha256_file, sha256_tagged};
use math_atoms_secrets::redact_sensitive_text;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

pub const MAX_PROVIDER_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PROVIDER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct ProviderHttpRequest<'a> {
    pub endpoint: &'a str,
    pub auth_header: &'a str,
    pub auth_scheme: &'a str,
    pub api_key: &'a str,
    pub body_json: &'a str,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderTransportError {
    Io(String),
    CurlFailed {
        code: Option<i32>,
        http_status: Option<u16>,
        stderr: String,
        body: String,
    },
    ResponseTooLarge,
}

impl fmt::Display for ProviderTransportError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(reason) => write!(output, "provider transport I/O failed: {reason}"),
            Self::CurlFailed {
                code,
                http_status,
                stderr,
                body,
            } => write!(
                output,
                "provider HTTP failed code={code:?} status={http_status:?} stderr={stderr} body={body}"
            ),
            Self::ResponseTooLarge => write!(output, "provider response exceeded the byte limit"),
        }
    }
}

impl std::error::Error for ProviderTransportError {}

impl From<io::Error> for ProviderTransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedProviderOutput {
    pub path: PathBuf,
    pub hash: String,
    pub len: usize,
}

pub fn post_json(request: ProviderHttpRequest<'_>) -> Result<String, ProviderTransportError> {
    let dir = std::env::temp_dir();
    let stem = format!(
        "math-atoms-provider-{}-{}",
        std::process::id(),
        unique_suffix()
    );
    let body_path = dir.join(format!("{stem}.json"));
    fs::write(&body_path, request.body_json)?;
    let body_arg = format!("@{}", body_path.to_string_lossy());
    let config = curl_config(
        request.endpoint,
        request.auth_header,
        request.auth_scheme,
        request.api_key,
    );
    let mut result = Err(ProviderTransportError::Io(
        "provider transport did not execute".to_string(),
    ));
    for attempt in 0..3 {
        let response_path = dir.join(format!("{stem}-response-{attempt}.json"));
        result = run_curl_fallback(&body_arg, &config, &response_path, request.timeout_seconds)
            .and_then(|capture| {
                let status_text = String::from_utf8_lossy(&capture.output.stdout);
                let http_status = split_http_status(&status_text);
                if !capture.output.status.success() {
                    return Err(ProviderTransportError::CurlFailed {
                        code: capture.output.status.code(),
                        http_status,
                        stderr: safe_diagnostic(
                            &String::from_utf8_lossy(&capture.output.stderr),
                            request.api_key,
                        ),
                        body: safe_diagnostic(&capture.body, request.api_key),
                    });
                }
                Ok(capture.body)
            });
        if !retryable(&result) || attempt == 2 {
            break;
        }
        thread::sleep(Duration::from_secs(1 << attempt));
    }
    if let Err(error) = fs::remove_file(&body_path) {
        return Err(ProviderTransportError::Io(format!(
            "provider temp cleanup failed: {error}"
        )));
    }
    result
}

pub fn provider_output_hash(text: &str) -> String {
    sha256_tagged(text.as_bytes())
}

pub fn default_provider_output_dir() -> PathBuf {
    let base = std::env::var("MATH_ATOMS_STORE_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("MathAtomsCoder").join("provider-outputs")
}

pub fn persist_provider_output(
    text: &str,
    directory: impl AsRef<Path>,
) -> io::Result<PersistedProviderOutput> {
    if text.trim().is_empty() || text.len() > MAX_PROVIDER_OUTPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provider output is empty or exceeds the evidence limit",
        ));
    }
    let hash = provider_output_hash(text);
    let hex = hash.strip_prefix("sha256:").unwrap_or(&hash);
    let directory = directory.as_ref();
    fs::create_dir_all(directory)?;
    let path = directory.join(format!("{hex}.txt"));
    if path.exists() {
        if sha256_file(&path)? != hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "content-addressed provider artifact hash mismatch",
            ));
        }
    } else {
        let temp = directory.join(format!(
            "{hex}.{}.{}.tmp",
            std::process::id(),
            unique_suffix()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        drop(file);
        match fs::rename(&temp, &path) {
            Ok(()) => {}
            Err(_) if path.exists() => {
                let _ = fs::remove_file(&temp);
                if sha256_file(&path)? != hash {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "existing provider artifact failed hash verification",
                    ));
                }
            }
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        }
    }
    Ok(PersistedProviderOutput {
        path,
        hash,
        len: text.len(),
    })
}

fn curl_config(endpoint: &str, auth_header: &str, auth_scheme: &str, api_key: &str) -> String {
    let auth_value = if auth_scheme.trim().is_empty() {
        api_key.to_string()
    } else {
        format!("{} {}", auth_scheme.trim(), api_key)
    };
    format!(
        "url = \"{}\"\nrequest = \"POST\"\nheader = \"{}: {}\"\nheader = \"Content-Type: application/json\"\n",
        curl_escape(endpoint),
        curl_escape(auth_header),
        curl_escape(&auth_value)
    )
}

fn curl_args(body_arg: &str, response_path: &Path, timeout_seconds: u64) -> Vec<String> {
    vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--fail-with-body".to_string(),
        "--connect-timeout".to_string(),
        "10".to_string(),
        "--max-time".to_string(),
        timeout_seconds.to_string(),
        "--output".to_string(),
        response_path.to_string_lossy().to_string(),
        "--write-out".to_string(),
        "\n__MATH_ATOMS_HTTP_STATUS__:%{http_code}".to_string(),
        "--config".to_string(),
        "-".to_string(),
        "--data-binary".to_string(),
        body_arg.to_string(),
    ]
}

struct CurlCapture {
    output: Output,
    body: String,
}

fn run_curl_fallback(
    body_arg: &str,
    config: &str,
    response_path: &Path,
    timeout_seconds: u64,
) -> Result<CurlCapture, ProviderTransportError> {
    match run_curl("curl.exe", body_arg, config, response_path, timeout_seconds) {
        Err(ProviderTransportError::Io(_)) => {
            run_curl("curl", body_arg, config, response_path, timeout_seconds)
        }
        result => result,
    }
}

fn run_curl(
    program: &str,
    body_arg: &str,
    config: &str,
    response_path: &Path,
    timeout_seconds: u64,
) -> Result<CurlCapture, ProviderTransportError> {
    let mut child = Command::new(program)
        .args(curl_args(body_arg, response_path, timeout_seconds))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "curl stdin unavailable").into());
    };
    stdin.write_all(config.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;
    let body_result = read_bounded_response(response_path, output.status.success());
    match fs::remove_file(response_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ProviderTransportError::Io(format!(
                "provider response temp cleanup failed: {error}"
            )))
        }
    }
    Ok(CurlCapture {
        output,
        body: body_result?,
    })
}

fn read_bounded_response(
    response_path: &Path,
    success: bool,
) -> Result<String, ProviderTransportError> {
    let size = match fs::metadata(response_path) {
        Ok(metadata) => metadata.len() as usize,
        Err(error) if !success && error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    if size > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(ProviderTransportError::ResponseTooLarge);
    }
    if size == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&fs::read(response_path)?).to_string())
}

fn retryable(result: &Result<String, ProviderTransportError>) -> bool {
    matches!(
        result,
        Err(ProviderTransportError::CurlFailed { code: Some(28), .. })
            | Err(ProviderTransportError::CurlFailed {
                http_status: Some(429 | 500..=599),
                ..
            })
    )
}

fn split_http_status(stdout: &str) -> Option<u16> {
    let marker = "\n__MATH_ATOMS_HTTP_STATUS__:";
    stdout
        .rfind(marker)
        .and_then(|pos| stdout[pos + marker.len()..].trim().parse::<u16>().ok())
}

fn safe_diagnostic(body: &str, api_key: &str) -> String {
    const MAX: usize = 700;
    let exact = if api_key.is_empty() {
        body.to_string()
    } else {
        body.replace(api_key, "[REDACTED]")
    };
    let redacted = redact_sensitive_text(&exact);
    if redacted.len() <= MAX {
        return redacted;
    }
    let mut end = MAX;
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &redacted[..end])
}

fn curl_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_configuration_keeps_secret_out_of_process_arguments() {
        let args = curl_args("@payload.json", Path::new("response.json"), 900);
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--config" && pair[1] == "-"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--max-time" && pair[1] == "900"));
        assert!(!args.iter().any(|arg| arg.contains("sk-test-secret")));
        let config = curl_config("https://example.invalid", "x-api-key", "", "sk-test-secret");
        assert!(config.contains("header = \"x-api-key: sk-test-secret\""));
    }

    #[test]
    fn bounded_reader_rejects_oversized_response_before_loading_it() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-oversized-response-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let file = fs::File::create(&path).unwrap();
        file.set_len((MAX_PROVIDER_RESPONSE_BYTES + 1) as u64)
            .unwrap();
        assert_eq!(
            read_bounded_response(&path, true),
            Err(ProviderTransportError::ResponseTooLarge)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn diagnostics_are_utf8_safe_and_redacted() {
        let body = format!("token = hunter2 {}", "é".repeat(800));
        let output = safe_diagnostic(&body, "");
        assert!(!output.contains("hunter2"));
        assert!(output.ends_with("..."));
    }

    #[test]
    fn output_artifact_is_content_addressed_and_recomputable() {
        let dir = std::env::temp_dir().join(format!(
            "math-atoms-provider-output-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let stored = persist_provider_output("provider proof", &dir).unwrap();
        assert_eq!(stored.hash, provider_output_hash("provider proof"));
        assert_eq!(sha256_file(&stored.path).unwrap(), stored.hash);
        assert_eq!(
            persist_provider_output("provider proof", &dir).unwrap(),
            stored
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
