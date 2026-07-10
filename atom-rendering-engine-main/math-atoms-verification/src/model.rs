use math_atoms_hash::sha256_tagged;
use math_atoms_secrets::redact_sensitive_text;
use std::fmt;
use std::path::PathBuf;

pub const VERIFICATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_CANDIDATE_FILES: usize = 32;
pub const MAX_VERIFICATION_ATTEMPTS: u32 = 32;
pub(crate) const MAX_CANDIDATE_FILE_BYTES: usize = 128 * 1024;
pub(crate) const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_FAILURE_BYTES: usize = 24 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateFile {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateProfile {
    Rust,
    Json,
}

impl CandidateFile {
    pub fn new(
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self, VerificationError> {
        let item = Self {
            path: path.into(),
            content: content.into(),
        };
        item.validate()?;
        Ok(item)
    }

    pub fn validate(&self) -> Result<(), VerificationError> {
        validate_relative_path(&self.path)?;
        if self.content.is_empty() {
            return Err(VerificationError::InvalidCandidate(format!(
                "candidate file {} is empty",
                self.path
            )));
        }
        if self.content.len() > MAX_CANDIDATE_FILE_BYTES {
            return Err(VerificationError::InvalidCandidate(format!(
                "candidate file {} exceeds {} bytes",
                self.path, MAX_CANDIDATE_FILE_BYTES
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationPolicy {
    pub command_timeout_seconds: u64,
}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self {
            command_timeout_seconds: 180,
        }
    }
}

impl VerificationPolicy {
    pub fn strict(command_timeout_seconds: u64) -> Result<Self, VerificationError> {
        if !(10..=1_800).contains(&command_timeout_seconds) {
            return Err(VerificationError::InvalidPolicy(
                "command timeout must be between 10 and 1800 seconds".to_string(),
            ));
        }
        Ok(Self {
            command_timeout_seconds,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandEvidence {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout_path: PathBuf,
    pub stdout_hash: String,
    pub stdout_len: usize,
    pub stderr_path: PathBuf,
    pub stderr_hash: String,
    pub stderr_len: usize,
}

impl CommandEvidence {
    pub fn passed(&self) -> bool {
        !self.timed_out && self.exit_code == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationAttempt {
    pub plan_id: String,
    pub attempt: u32,
    pub passed: bool,
    pub candidate_dir: PathBuf,
    pub bundle_hash: String,
    pub files: Vec<FileEvidence>,
    pub commands: Vec<CommandEvidence>,
    pub failure: String,
    pub manifest_path: PathBuf,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairEvidence {
    pub plan_id: String,
    pub after_attempt: u32,
    pub source_bundle_hash: String,
    pub repaired_bundle_hash: String,
    pub model: String,
    pub files: Vec<CandidateFile>,
    pub manifest_path: PathBuf,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEvidence {
    pub path: String,
    pub hash: String,
    pub len: usize,
    pub controller_owned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationSuccess {
    pub plan_id: String,
    pub attempts: u32,
    pub repairs: u32,
    pub bundle_hash: String,
    pub candidate_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CandidateVerificationEvidence {
    pub manifest_path: String,
    pub manifest_hash: String,
    pub bundle_hash: String,
    pub attempts: u32,
    pub repairs: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCandidate {
    pub plan_id: String,
    pub attempts: u32,
    pub repairs: u32,
    pub bundle_hash: String,
    pub candidate_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationError {
    InvalidPolicy(String),
    InvalidCandidate(String),
    UnsupportedCandidate(String),
    Command(String),
    Evidence(String),
    Io(String),
}

impl fmt::Display for VerificationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy(reason) => write!(output, "invalid verification policy: {reason}"),
            Self::InvalidCandidate(reason) => write!(output, "invalid candidate: {reason}"),
            Self::UnsupportedCandidate(reason) => write!(output, "unsupported candidate: {reason}"),
            Self::Command(reason) => write!(output, "candidate command failed: {reason}"),
            Self::Evidence(reason) => write!(output, "candidate evidence failed: {reason}"),
            Self::Io(reason) => write!(output, "candidate verification I/O failed: {reason}"),
        }
    }
}

impl std::error::Error for VerificationError {}

impl From<std::io::Error> for VerificationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(redact_sensitive_text(&error.to_string()))
    }
}

pub(crate) fn validate_plan_id(value: &str) -> Result<(), VerificationError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(VerificationError::InvalidCandidate(format!(
            "unsafe plan id: {value}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_relative_path(value: &str) -> Result<(), VerificationError> {
    let normalized = value.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    if normalized.is_empty()
        || normalized.len() > 240
        || path.is_absolute()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.chars().any(|ch| ch.is_control())
        })
    {
        return Err(VerificationError::InvalidCandidate(format!(
            "unsafe candidate path: {value}"
        )));
    }
    Ok(())
}

pub fn candidate_output(files: &[CandidateFile]) -> Result<String, VerificationError> {
    validate_files(files)?;
    if files.len() == 1 {
        return Ok(files[0].content.clone());
    }
    let mut output = String::new();
    for file in files {
        output.push_str("FILE: ");
        output.push_str(&file.path.replace('\\', "/"));
        output.push('\n');
        output.push_str(&file.content);
        if !file.content.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output)
}

pub(crate) fn bundle_hash(files: &[CandidateFile]) -> Result<String, VerificationError> {
    Ok(sha256_tagged(candidate_output(files)?.as_bytes()))
}

pub(crate) fn validate_files(files: &[CandidateFile]) -> Result<(), VerificationError> {
    if files.is_empty() || files.len() > MAX_CANDIDATE_FILES {
        return Err(VerificationError::InvalidCandidate(format!(
            "candidate requires 1 to {MAX_CANDIDATE_FILES} files"
        )));
    }
    let mut paths = std::collections::HashSet::new();
    for file in files {
        file.validate()?;
        let normalized = file.path.replace('\\', "/").to_ascii_lowercase();
        if !paths.insert(normalized) {
            return Err(VerificationError::InvalidCandidate(format!(
                "duplicate candidate path: {}",
                file.path
            )));
        }
    }
    Ok(())
}

pub(crate) fn candidate_profile(
    files: &[CandidateFile],
) -> Result<CandidateProfile, VerificationError> {
    validate_files(files)?;
    let has_manifest = files.iter().any(|file| {
        file.path
            .replace('\\', "/")
            .eq_ignore_ascii_case("Cargo.toml")
    });
    let rust_files = files
        .iter()
        .filter(|file| file.path.to_ascii_lowercase().ends_with(".rs"))
        .count();
    if has_manifest || rust_files > 0 {
        return Ok(CandidateProfile::Rust);
    }
    if files
        .iter()
        .all(|file| file.path.to_ascii_lowercase().ends_with(".json"))
    {
        return Ok(CandidateProfile::Json);
    }
    Err(VerificationError::UnsupportedCandidate(
        "candidate must be a Rust crate/source bundle or a JSON artifact bundle".to_string(),
    ))
}

pub(crate) fn json_validation_failure(
    files: &[CandidateFile],
) -> Result<Option<String>, VerificationError> {
    if candidate_profile(files)? != CandidateProfile::Json {
        return Ok(None);
    }
    for file in files {
        if let Err(error) = math_atoms_json::parse(&file.content) {
            return Ok(Some(clean_failure(&format!(
                "JSON file {} failed strict parsing: {error}",
                file.path
            ))));
        }
    }
    Ok(None)
}

pub(crate) fn controller_json_harness(
    files: &[CandidateFile],
) -> Result<String, VerificationError> {
    if let Some(failure) = json_validation_failure(files)? {
        return Ok(format!(
            "#![forbid(unsafe_code)]\ncompile_error!({failure:?});\n"
        ));
    }
    let bytes = files.iter().map(|file| file.content.len()).sum::<usize>();
    Ok(format!(
        "#![forbid(unsafe_code)]\npub const VERIFIED_JSON_FILES: usize = {};\npub const VERIFIED_JSON_BYTES: usize = {bytes};\n#[cfg(test)]\nmod tests {{\n    use super::*;\n    #[test]\n    fn controller_validated_nonempty_json_bundle() {{\n        assert!(std::hint::black_box(VERIFIED_JSON_FILES) > 0);\n        assert!(std::hint::black_box(VERIFIED_JSON_BYTES) > 0);\n    }}\n}}\n",
        files.len()
    ))
}

pub(crate) fn controller_manifest(extra: &str) -> String {
    format!(
        "[package]\nname = \"atom-verified-candidate\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n{extra}"
    )
}

pub(crate) fn clean_failure(value: &str) -> String {
    let redacted = redact_sensitive_text(value);
    if redacted.len() <= MAX_FAILURE_BYTES {
        return redacted;
    }
    let mut end = MAX_FAILURE_BYTES;
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[verification output truncated]", &redacted[..end])
}
