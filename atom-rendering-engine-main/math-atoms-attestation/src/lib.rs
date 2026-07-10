//! Typed, immutable evidence produced by executing an allowlisted functional harness.

use math_atoms_hash::{sha256_file, sha256_tagged, valid_sha256_tag};
use math_atoms_json::{parse as parse_json, JsonValue};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const HARNESS_SCHEMA_VERSION: u32 = 1;
const MAX_CAPTURE_BYTES: u64 = 1024 * 1024;
const ASSERTIONS: [&str; 5] = [
    "allowlisted-harness",
    "exit-code-zero",
    "stdout-exact",
    "artifact-hash-recomputed",
    "executable-hash-recomputed",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRunSpec {
    pub harness_id: String,
    pub gate: String,
    pub work_plan_id: String,
    pub provider_model: String,
    pub artifact_path: PathBuf,
    pub executable_path: PathBuf,
    pub working_directory: PathBuf,
    pub expected_stdout: String,
    pub artifact_env: Option<String>,
    pub timeout_seconds: u64,
    pub attestation_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessExpectation<'a> {
    pub gate: &'a str,
    pub work_plan_id: &'a str,
    pub provider_model: &'a str,
    pub artifact_path: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessAttestation {
    pub schema_version: u32,
    pub harness_id: String,
    pub timestamp_ms: u64,
    pub gate: String,
    pub work_plan_id: String,
    pub provider_model: String,
    pub artifact_path: String,
    pub artifact_hash: String,
    pub executable_path: String,
    pub executable_hash: String,
    pub working_directory: String,
    pub command: String,
    pub artifact_env: String,
    pub exit_code: i32,
    pub expected_stdout_hash: String,
    pub actual_stdout_hash: String,
    pub stderr_hash: String,
    pub assertions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrittenAttestation {
    pub path: PathBuf,
    pub hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttestationError {
    Invalid(String),
    Execution(String),
    Io(String),
}

impl fmt::Display for AttestationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(output, "invalid harness attestation: {reason}"),
            Self::Execution(reason) => write!(output, "harness execution failed: {reason}"),
            Self::Io(reason) => write!(output, "harness attestation I/O failed: {reason}"),
        }
    }
}

impl std::error::Error for AttestationError {}

impl From<io::Error> for AttestationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub fn run_harness(spec: &HarnessRunSpec) -> Result<WrittenAttestation, AttestationError> {
    if !allowlisted_harness(&spec.harness_id) {
        return Err(AttestationError::Invalid(format!(
            "harness is not allowlisted: {}",
            spec.harness_id
        )));
    }
    if spec.gate.trim().is_empty()
        || spec.expected_stdout.trim().is_empty()
        || !(1..=600).contains(&spec.timeout_seconds)
    {
        return Err(AttestationError::Invalid(
            "gate, expected stdout, or timeout is invalid".to_string(),
        ));
    }
    let executable = spec.executable_path.canonicalize()?;
    let working_directory = spec.working_directory.canonicalize()?;
    let mut command = Command::new(&executable);
    command
        .current_dir(&working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let artifact_env = expected_artifact_env(&spec.harness_id);
    if spec.artifact_env.as_deref() != artifact_env {
        return Err(AttestationError::Invalid(
            "artifact environment does not match the allowlisted harness".to_string(),
        ));
    }
    if let Some(name) = artifact_env {
        command.env(name, &spec.artifact_path);
    }
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AttestationError::Execution("stdout pipe is missing".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AttestationError::Execution("stderr pipe is missing".to_string()))?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));
    let deadline = Instant::now() + Duration::from_secs(spec.timeout_seconds);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AttestationError::Execution(format!(
                "harness exceeded {} seconds",
                spec.timeout_seconds
            )));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = join_reader(stdout_reader, "stdout")?;
    let stderr = join_reader(stderr_reader, "stderr")?;
    let actual = String::from_utf8(stdout)
        .map_err(|_| AttestationError::Execution("harness stdout is not UTF-8".to_string()))?;
    let expected = canonical_stdout(&spec.expected_stdout);
    let actual = canonical_stdout(&actual);
    let exit_code = status.code().unwrap_or(-1);
    if exit_code != 0 || actual != expected {
        return Err(AttestationError::Execution(format!(
            "exit={exit_code}, stdout did not match the exact functional contract"
        )));
    }
    let artifact = spec.artifact_path.canonicalize()?;
    let artifact_hash = sha256_file(&artifact)?;
    let executable_hash = sha256_file(&executable)?;
    let attestation = HarnessAttestation {
        schema_version: HARNESS_SCHEMA_VERSION,
        harness_id: spec.harness_id.clone(),
        timestamp_ms: now_ms(),
        gate: spec.gate.clone(),
        work_plan_id: spec.work_plan_id.clone(),
        provider_model: spec.provider_model.clone(),
        artifact_path: artifact.to_string_lossy().to_string(),
        artifact_hash,
        executable_path: executable.to_string_lossy().to_string(),
        executable_hash,
        working_directory: working_directory.to_string_lossy().to_string(),
        command: executable.to_string_lossy().to_string(),
        artifact_env: artifact_env.unwrap_or("").to_string(),
        exit_code,
        expected_stdout_hash: sha256_tagged(expected.as_bytes()),
        actual_stdout_hash: sha256_tagged(actual.as_bytes()),
        stderr_hash: sha256_tagged(&stderr),
        assertions: ASSERTIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    };
    attestation.validate()?;
    write_immutable(&spec.attestation_path, attestation.to_json().as_bytes())?;
    Ok(WrittenAttestation {
        path: spec.attestation_path.canonicalize()?,
        hash: sha256_file(&spec.attestation_path)?,
    })
}

pub fn verify_harness_attestation(
    path: impl AsRef<Path>,
    expected_hash: &str,
    expectation: HarnessExpectation<'_>,
) -> Result<HarnessAttestation, AttestationError> {
    if !valid_sha256_tag(expected_hash) {
        return Err(AttestationError::Invalid(
            "attestation hash is invalid".to_string(),
        ));
    }
    let path = path.as_ref().canonicalize()?;
    if sha256_file(&path)? != expected_hash {
        return Err(AttestationError::Invalid(
            "attestation hash does not recompute".to_string(),
        ));
    }
    let attestation = HarnessAttestation::from_json(&fs::read_to_string(path)?)?;
    attestation.validate()?;
    let expected_artifact = Path::new(expectation.artifact_path).canonicalize()?;
    let attested_artifact = Path::new(&attestation.artifact_path).canonicalize()?;
    if attestation.gate != expectation.gate
        || attestation.work_plan_id != expectation.work_plan_id
        || attestation.provider_model != expectation.provider_model
        || attested_artifact != expected_artifact
        || sha256_file(&attested_artifact)? != attestation.artifact_hash
        || sha256_file(&attestation.executable_path)? != attestation.executable_hash
    {
        return Err(AttestationError::Invalid(
            "attestation does not match the learning record or live artifacts".to_string(),
        ));
    }
    Ok(attestation)
}

impl HarnessAttestation {
    fn validate(&self) -> Result<(), AttestationError> {
        if self.schema_version != HARNESS_SCHEMA_VERSION
            || !allowlisted_harness(&self.harness_id)
            || self.timestamp_ms == 0
            || self.gate.trim().is_empty()
            || self.artifact_path.trim().is_empty()
            || self.executable_path.trim().is_empty()
            || self.command != self.executable_path
            || self.artifact_env != expected_artifact_env(&self.harness_id).unwrap_or("")
            || self.exit_code != 0
            || self.expected_stdout_hash != self.actual_stdout_hash
            || self.assertions != ASSERTIONS
            || [
                &self.artifact_hash,
                &self.executable_hash,
                &self.expected_stdout_hash,
                &self.actual_stdout_hash,
                &self.stderr_hash,
            ]
            .iter()
            .any(|hash| !valid_sha256_tag(hash))
        {
            return Err(AttestationError::Invalid(
                "attestation fields do not satisfy the strict schema".to_string(),
            ));
        }
        Ok(())
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":{},\"harness_id\":\"{}\",\"timestamp_ms\":{},\"gate\":\"{}\",\"work_plan_id\":\"{}\",\"provider_model\":\"{}\",\"artifact_path\":\"{}\",\"artifact_hash\":\"{}\",\"executable_path\":\"{}\",\"executable_hash\":\"{}\",\"working_directory\":\"{}\",\"command\":\"{}\",\"artifact_env\":\"{}\",\"exit_code\":{},\"expected_stdout_hash\":\"{}\",\"actual_stdout_hash\":\"{}\",\"stderr_hash\":\"{}\",\"assertions\":[{}]}}",
            self.schema_version,
            escape(&self.harness_id),
            self.timestamp_ms,
            escape(&self.gate),
            escape(&self.work_plan_id),
            escape(&self.provider_model),
            escape(&self.artifact_path),
            escape(&self.artifact_hash),
            escape(&self.executable_path),
            escape(&self.executable_hash),
            escape(&self.working_directory),
            escape(&self.command),
            escape(&self.artifact_env),
            self.exit_code,
            escape(&self.expected_stdout_hash),
            escape(&self.actual_stdout_hash),
            escape(&self.stderr_hash),
            self.assertions
                .iter()
                .map(|value| format!("\"{}\"", escape(value)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn from_json(input: &str) -> Result<Self, AttestationError> {
        let root = parse_json(input)
            .map_err(|error| AttestationError::Invalid(format!("JSON: {error}")))?;
        let object = root
            .as_object()
            .ok_or_else(|| AttestationError::Invalid("root is not an object".to_string()))?;
        let expected: HashSet<&str> = [
            "schema_version",
            "harness_id",
            "timestamp_ms",
            "gate",
            "work_plan_id",
            "provider_model",
            "artifact_path",
            "artifact_hash",
            "executable_path",
            "executable_hash",
            "working_directory",
            "command",
            "artifact_env",
            "exit_code",
            "expected_stdout_hash",
            "actual_stdout_hash",
            "stderr_hash",
            "assertions",
        ]
        .into_iter()
        .collect();
        let actual: HashSet<&str> = object.iter().map(|(key, _)| key.as_str()).collect();
        if actual != expected {
            return Err(AttestationError::Invalid(
                "fields are not the exact schema".to_string(),
            ));
        }
        Ok(Self {
            schema_version: number(&root, "schema_version")? as u32,
            harness_id: string(&root, "harness_id")?,
            timestamp_ms: number(&root, "timestamp_ms")?,
            gate: string(&root, "gate")?,
            work_plan_id: string(&root, "work_plan_id")?,
            provider_model: string(&root, "provider_model")?,
            artifact_path: string(&root, "artifact_path")?,
            artifact_hash: string(&root, "artifact_hash")?,
            executable_path: string(&root, "executable_path")?,
            executable_hash: string(&root, "executable_hash")?,
            working_directory: string(&root, "working_directory")?,
            command: string(&root, "command")?,
            artifact_env: string(&root, "artifact_env")?,
            exit_code: root
                .get("exit_code")
                .and_then(JsonValue::as_u64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| AttestationError::Invalid("exit_code is invalid".to_string()))?,
            expected_stdout_hash: string(&root, "expected_stdout_hash")?,
            actual_stdout_hash: string(&root, "actual_stdout_hash")?,
            stderr_hash: string(&root, "stderr_hash")?,
            assertions: strings(&root, "assertions")?,
        })
    }
}

fn allowlisted_harness(value: &str) -> bool {
    matches!(
        value,
        "rust-console-exact-v1"
            | "native-pmre-functional-v1"
            | "design-upload-functional-v1"
            | "provider-transport-functional-v1"
            | "self-learning-restart-v1"
    )
}

fn expected_artifact_env(harness_id: &str) -> Option<&'static str> {
    match harness_id {
        "native-pmre-functional-v1" => Some("MATH_ATOMS_REAL_APP_BMP"),
        "design-upload-functional-v1" => Some("MATH_ATOMS_DESIGN_APP_BMP"),
        "provider-transport-functional-v1" => Some("MATH_ATOMS_PROVIDER_OUTPUT"),
        _ => None,
    }
}

fn read_bounded<R: Read>(reader: R) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "harness output exceeded the byte limit",
        ));
    }
    Ok(bytes)
}

fn join_reader(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    label: &str,
) -> Result<Vec<u8>, AttestationError> {
    reader
        .join()
        .map_err(|_| AttestationError::Execution(format!("{label} reader panicked")))?
        .map_err(AttestationError::from)
}

fn canonical_stdout(value: &str) -> String {
    value.trim_end_matches(['\r', '\n']).to_string()
}

fn write_immutable(path: &Path, bytes: &[u8]) -> Result<(), AttestationError> {
    if path.exists() {
        return Err(AttestationError::Io(format!(
            "attestation path already exists: {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| AttestationError::Io("attestation has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if fs::read(path)? != bytes {
        return Err(AttestationError::Io(
            "attestation readback mismatch".to_string(),
        ));
    }
    Ok(())
}

fn string(root: &JsonValue, key: &str) -> Result<String, AttestationError> {
    root.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| AttestationError::Invalid(format!("{key} is not a string")))
}

fn strings(root: &JsonValue, key: &str) -> Result<Vec<String>, AttestationError> {
    root.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| AttestationError::Invalid(format!("{key} is not an array")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| AttestationError::Invalid(format!("{key} entry is invalid")))
        })
        .collect()
}

fn number(root: &JsonValue, key: &str) -> Result<u64, AttestationError> {
    root.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| AttestationError::Invalid(format!("{key} is not an unsigned integer")))
}

fn escape(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push(' '),
            ch => output.push(ch),
        }
    }
    output
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_harness_is_rejected_before_execution() {
        let spec = HarnessRunSpec {
            harness_id: "caller-selected".to_string(),
            gate: "test".to_string(),
            work_plan_id: String::new(),
            provider_model: String::new(),
            artifact_path: PathBuf::from("missing"),
            executable_path: PathBuf::from("missing"),
            working_directory: PathBuf::from("missing"),
            expected_stdout: "ok".to_string(),
            artifact_env: None,
            timeout_seconds: 10,
            attestation_path: PathBuf::from("missing"),
        };
        assert!(matches!(
            run_harness(&spec),
            Err(AttestationError::Invalid(_))
        ));
    }
}
