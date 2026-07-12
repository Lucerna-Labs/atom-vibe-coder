//! Bounded credential-safe provider HTTP transport and content-addressed output evidence.

use math_atoms_hash::{sha256_file, sha256_tagged};
use math_atoms_secrets::redact_sensitive_text;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Windows `CREATE_NO_WINDOW`: keeps the `curl` subprocess from flashing a black console
/// window over the GUI on every provider call.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
use std::thread;

pub const MAX_PROVIDER_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PROVIDER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CURL_STDERR_BYTES: usize = 64 * 1024;
const STATUS_MARKER: &str = "\n__MATH_ATOMS_HTTP_STATUS__:";

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

struct TempBody {
    path: PathBuf,
    armed: bool,
}

impl TempBody {
    fn create(path: PathBuf, content: &str) -> io::Result<Self> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let temp = Self { path, armed: true };
        file.write_all(content.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        drop(file);
        Ok(temp)
    }

    fn cleanup(mut self) -> io::Result<()> {
        fs::remove_file(&self.path)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempBody {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn post_json(request: ProviderHttpRequest<'_>) -> Result<String, ProviderTransportError> {
    if request.body_json.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(ProviderTransportError::ResponseTooLarge);
    }
    validate_curl_field(request.endpoint)?;
    validate_curl_field(request.auth_header)?;
    validate_curl_field(request.auth_scheme)?;
    validate_curl_field(request.api_key)?;
    let dir = std::env::temp_dir();
    let stem = format!(
        "math-atoms-provider-{}-{}",
        std::process::id(),
        unique_suffix()
    );
    let body_path = dir.join(format!("{stem}.json"));
    let temp_body = TempBody::create(body_path, request.body_json)?;
    let body_arg = format!("@{}", temp_body.path.to_string_lossy());
    let config = curl_config(
        request.endpoint,
        request.auth_header,
        request.auth_scheme,
        request.api_key,
    );
    // Retry ONLY the class of failure where curl never issued the request — the process
    // failed to launch/initialize (e.g. Windows STATUS_DLL_INIT_FAILED 0xC0000142 under
    // resource pressure). Such a retry cannot double-submit, and it keeps one transient
    // hiccup on a late packet from discarding an entire multi-packet build. HTTP-status
    // failures (4xx/5xx) and curl's own request-level errors are never retried.
    const MAX_ATTEMPTS: u32 = 3;
    let mut attempt = 0u32;
    let result = loop {
        attempt += 1;
        let outcome = run_curl(&body_arg, &config, request.timeout_seconds)
            .and_then(|capture| classify_curl_capture(capture, request.api_key));
        match &outcome {
            Err(error) if attempt < MAX_ATTEMPTS && is_transient_launch_failure(error) => {
                std::thread::sleep(std::time::Duration::from_millis(250 * u64::from(attempt)));
            }
            _ => break outcome,
        }
    };
    match temp_body.cleanup() {
        Ok(()) => result,
        Err(error) => Err(ProviderTransportError::Io(format!(
            "provider temp cleanup failed: {error}"
        ))),
    }
}

/// Turn a completed curl invocation into the response body or a classified error.
/// When curl itself failed (timeout, connection reset, aborted transfer) it writes no
/// `--write-out` status marker, so surface its real exit code and stderr — e.g.
/// "curl: (52) Empty reply from server" — instead of the misleading "omitted the HTTP
/// status marker", which otherwise hides the true cause. A present marker is always
/// preferred (curl `--fail` still emits it with the real HTTP code).
fn classify_curl_capture(
    capture: CurlCapture,
    api_key: &str,
) -> Result<String, ProviderTransportError> {
    let stderr = String::from_utf8(capture.stderr)
        .map_err(|_| ProviderTransportError::Io("curl stderr was not valid UTF-8".to_string()))?;
    let curl_failed = !capture.status.success();
    let curl_code = capture.status.code();
    match split_curl_response(capture.stdout) {
        Ok((body, http_status)) => {
            if curl_failed || !matches!(http_status, Some(200..=299)) {
                return Err(ProviderTransportError::CurlFailed {
                    code: curl_code,
                    http_status,
                    stderr: safe_diagnostic(&stderr, api_key),
                    body: safe_diagnostic(&body, api_key),
                });
            }
            Ok(body)
        }
        Err(framing_error) => {
            if curl_failed {
                Err(ProviderTransportError::CurlFailed {
                    code: curl_code,
                    http_status: None,
                    stderr: safe_diagnostic(&stderr, api_key),
                    body: String::new(),
                })
            } else {
                Err(framing_error)
            }
        }
    }
}

/// True only for a curl failure where the process never issued the HTTP request, so a
/// retry cannot double-submit: no HTTP status was obtained AND the exit code is an
/// abnormal OS-level termination (outside curl's documented 1..=99 range) or absent —
/// e.g. Windows STATUS_DLL_INIT_FAILED (0xC0000142) when the process fails to start.
/// curl's own request-level exit codes (1..=99) mean the request may have been sent and
/// are deliberately NOT retried.
fn is_transient_launch_failure(error: &ProviderTransportError) -> bool {
    match error {
        ProviderTransportError::CurlFailed {
            http_status: None,
            code,
            ..
        } => match code {
            None => true,
            Some(c) => !(1..=99).contains(c),
        },
        _ => false,
    }
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

fn curl_args(body_arg: &str, timeout_seconds: u64) -> Vec<String> {
    vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--fail-with-body".to_string(),
        "--connect-timeout".to_string(),
        "10".to_string(),
        "--max-time".to_string(),
        timeout_seconds.to_string(),
        "--write-out".to_string(),
        format!("{STATUS_MARKER}%{{http_code}}"),
        "--config".to_string(),
        "-".to_string(),
        "--data-binary".to_string(),
        body_arg.to_string(),
    ]
}

struct CurlCapture {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_curl(
    body_arg: &str,
    config: &str,
    timeout_seconds: u64,
) -> Result<CurlCapture, ProviderTransportError> {
    let mut command = Command::new(curl_program());
    command
        .args(curl_args(body_arg, timeout_seconds))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "curl stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "curl stderr unavailable"))?;
    let stdout_reader = thread::spawn(move || {
        read_bounded_stream(
            stdout,
            MAX_PROVIDER_RESPONSE_BYTES + STATUS_MARKER.len() + 3,
        )
    });
    let stderr_reader = thread::spawn(move || read_bounded_stream(stderr, MAX_CURL_STDERR_BYTES));
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "curl stdin unavailable").into());
    };
    if let Err(error) = stdin.write_all(config.as_bytes()) {
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        return Err(error.into());
    }
    drop(stdin);
    let status = child.wait()?;
    let stdout = join_reader(stdout_reader, "stdout")?;
    let stderr = join_reader(stderr_reader, "stderr")?;
    Ok(CurlCapture {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded_stream(
    mut input: impl Read,
    limit: usize,
) -> Result<Vec<u8>, ProviderTransportError> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let count = input.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > limit {
            return Err(ProviderTransportError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, ProviderTransportError>>,
    label: &str,
) -> Result<Vec<u8>, ProviderTransportError> {
    reader
        .join()
        .map_err(|_| ProviderTransportError::Io(format!("curl {label} reader panicked")))?
}

fn split_curl_response(stdout: Vec<u8>) -> Result<(String, Option<u16>), ProviderTransportError> {
    let stdout = String::from_utf8(stdout).map_err(|_| {
        ProviderTransportError::Io("provider response was not valid UTF-8".to_string())
    })?;
    let position = stdout.rfind(STATUS_MARKER).ok_or_else(|| {
        ProviderTransportError::Io("curl response omitted the HTTP status marker".to_string())
    })?;
    let status = stdout[position + STATUS_MARKER.len()..]
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|value| (100..=599).contains(value));
    Ok((stdout[..position].to_string(), status))
}

fn validate_curl_field(value: &str) -> Result<(), ProviderTransportError> {
    if value.chars().any(char::is_control) {
        return Err(ProviderTransportError::Io(
            "provider transport field contains control characters".to_string(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn curl_program() -> &'static str {
    "curl.exe"
}

#[cfg(not(windows))]
fn curl_program() -> &'static str {
    "curl"
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
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn read_request(stream: &mut std::net::TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).unwrap();
            bytes.extend_from_slice(&chunk[..count]);
        }
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut received = bytes.len() - header_end;
        while received < content_length {
            let count = stream.read(&mut chunk).unwrap();
            received += count;
        }
    }

    fn request(endpoint: &str) -> ProviderHttpRequest<'_> {
        ProviderHttpRequest {
            endpoint,
            auth_header: "Authorization",
            auth_scheme: "Bearer",
            api_key: "fixture-secret",
            body_json: "{\"probe\":true}",
            timeout_seconds: 10,
        }
    }

    #[test]
    fn curl_configuration_keeps_secret_out_of_process_arguments() {
        let args = curl_args("@payload.json", 900);
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
    fn bounded_reader_rejects_oversized_stream_without_a_response_file() {
        let input = io::Cursor::new(vec![b'x'; 1025]);
        assert_eq!(
            read_bounded_stream(input, 1024),
            Err(ProviderTransportError::ResponseTooLarge)
        );
    }

    #[test]
    fn response_framing_requires_strict_utf8_and_valid_status() {
        assert_eq!(
            split_curl_response(vec![0xff, b'\n']),
            Err(ProviderTransportError::Io(
                "provider response was not valid UTF-8".to_string()
            ))
        );
        let framed = format!("{{\"ok\":true}}{STATUS_MARKER}200").into_bytes();
        assert_eq!(
            split_curl_response(framed).unwrap(),
            ("{\"ok\":true}".to_string(), Some(200))
        );
        assert!(split_curl_response(b"missing marker".to_vec()).is_err());
    }

    #[test]
    fn live_transport_aborts_an_oversized_response_without_spooling_to_disk() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_PROVIDER_RESPONSE_BYTES + 1024
            )
            .unwrap();
            let chunk = [b'x'; 64 * 1024];
            let mut sent = 0;
            while sent <= MAX_PROVIDER_RESPONSE_BYTES + 1024 {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
                sent += chunk.len();
            }
        });
        assert_eq!(
            post_json(request(&format!("http://{address}/oversized"))),
            Err(ProviderTransportError::ResponseTooLarge)
        );
        server.join().unwrap();
    }

    #[test]
    fn live_transport_submits_a_failed_post_exactly_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let submissions = Arc::new(AtomicUsize::new(0));
        let observed = submissions.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            read_request(&mut stream);
            let body = b"{\"error\":\"fixture\"}";
            write!(
                stream,
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();
            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((_, _)) => {
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("listener failed: {error}"),
                }
            }
        });
        assert!(matches!(
            post_json(request(&format!("http://{address}/single-submit"))),
            Err(ProviderTransportError::CurlFailed {
                http_status: Some(500),
                ..
            })
        ));
        server.join().unwrap();
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn transient_launch_failures_are_classified_for_retry() {
        // Process never ran (STATUS_DLL_INIT_FAILED / abnormal OS exit): safe to retry.
        assert!(is_transient_launch_failure(
            &ProviderTransportError::CurlFailed {
                code: Some(-1_073_741_502),
                http_status: None,
                stderr: String::new(),
                body: String::new(),
            }
        ));
        assert!(is_transient_launch_failure(
            &ProviderTransportError::CurlFailed {
                code: None,
                http_status: None,
                stderr: String::new(),
                body: String::new(),
            }
        ));
        // HTTP 500 was submitted exactly once — must NOT be retried.
        assert!(!is_transient_launch_failure(
            &ProviderTransportError::CurlFailed {
                code: Some(22),
                http_status: Some(500),
                stderr: String::new(),
                body: String::new(),
            }
        ));
        // curl exit 52 (empty reply) means the request was sent — must NOT be retried.
        assert!(!is_transient_launch_failure(
            &ProviderTransportError::CurlFailed {
                code: Some(52),
                http_status: None,
                stderr: String::new(),
                body: String::new(),
            }
        ));
    }

    #[test]
    fn curl_transport_failure_surfaces_exit_code_not_missing_marker() {
        // The peer accepts then closes without any HTTP response, so curl fails
        // (e.g. "curl: (52) Empty reply from server") and writes no status marker.
        // OPERATOR_APPROVED_CPU_PARALLEL: test-only TCP server thread, identical to the
        // existing live_transport_* tests in this module (not product CPU parallelism).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        let result = post_json(request(&format!("http://{address}/reset")));
        server.join().unwrap();
        // The real curl exit code must surface, not the misleading framing error.
        match result {
            Err(ProviderTransportError::CurlFailed {
                http_status, code, ..
            }) => {
                assert_eq!(http_status, None);
                assert!(code.is_some(), "expected a concrete curl exit code");
            }
            other => panic!("expected CurlFailed carrying the curl exit code, got {other:?}"),
        }
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
