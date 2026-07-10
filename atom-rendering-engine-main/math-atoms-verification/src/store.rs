use crate::model::{
    validate_plan_id, CommandEvidence, FileEvidence, VerificationAttempt, VerificationError,
    VerificationSuccess, VerifiedCandidate, VERIFICATION_SCHEMA_VERSION,
};
use math_atoms_hash::{sha256_file, valid_sha256_tag};
use math_atoms_json::{parse as parse_json, JsonValue};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn verification_attempt_dir(root: &Path, plan_id: &str, attempt: u32) -> PathBuf {
    root.join(plan_id)
        .join("candidate-verification")
        .join(format!("attempt-{attempt:03}"))
}

pub(crate) fn write_attempt_manifest(
    attempt_dir: &Path,
    attempt: &VerificationAttempt,
    _normalized_files: &[crate::CandidateFile],
) -> Result<(PathBuf, String), VerificationError> {
    let path = attempt_dir.join("attempt.json");
    let files = attempt
        .files
        .iter()
        .map(file_json)
        .collect::<Vec<_>>()
        .join(",");
    let commands = attempt
        .commands
        .iter()
        .map(command_json)
        .collect::<Vec<_>>()
        .join(",");
    let text = format!(
        "{{\"schema_version\":{},\"plan_id\":\"{}\",\"attempt\":{},\"passed\":{},\"candidate_dir\":\"{}\",\"bundle_hash\":\"{}\",\"files\":[{}],\"commands\":[{}],\"failure\":\"{}\"}}",
        VERIFICATION_SCHEMA_VERSION,
        escape(&attempt.plan_id),
        attempt.attempt,
        attempt.passed,
        escape(&attempt.candidate_dir.to_string_lossy()),
        escape(&attempt.bundle_hash),
        files,
        commands,
        escape(&attempt.failure)
    );
    write_immutable_verified(&path, text.as_bytes())?;
    Ok((path.canonicalize()?, sha256_file(&path)?))
}

pub(crate) fn load_attempt(
    root: &Path,
    plan_id: &str,
    attempt: u32,
) -> Result<Option<VerificationAttempt>, VerificationError> {
    let path = verification_attempt_dir(root, plan_id, attempt).join("attempt.json");
    if !path.exists() {
        return Ok(None);
    }
    let hash = sha256_file(&path)?;
    let mut parsed = parse_attempt(&path)?;
    parsed.manifest_path = path.canonicalize()?;
    parsed.manifest_hash = hash;
    verify_attempt(&parsed)?;
    Ok(Some(parsed))
}

pub(crate) fn finalize_success(
    root: &Path,
    attempt: &VerificationAttempt,
) -> Result<VerificationSuccess, VerificationError> {
    verify_attempt(attempt)?;
    if !attempt.passed {
        return Err(VerificationError::Evidence(
            "final verification attempt did not pass".to_string(),
        ));
    }
    let dir = root.join(&attempt.plan_id).join("candidate-verification");
    fs::create_dir_all(&dir)?;
    let path = dir.join("verification-final.json");
    let text = format!(
        "{{\"schema_version\":{},\"plan_id\":\"{}\",\"passed\":true,\"attempts\":{},\"bundle_hash\":\"{}\",\"candidate_dir\":\"{}\",\"attempt_manifest\":\"{}\",\"attempt_manifest_hash\":\"{}\"}}",
        VERIFICATION_SCHEMA_VERSION,
        escape(&attempt.plan_id),
        attempt.attempt,
        escape(&attempt.bundle_hash),
        escape(&attempt.candidate_dir.to_string_lossy()),
        escape(&attempt.manifest_path.to_string_lossy()),
        escape(&attempt.manifest_hash)
    );
    write_immutable_verified(&path, text.as_bytes())?;
    Ok(VerificationSuccess {
        plan_id: attempt.plan_id.clone(),
        attempts: attempt.attempt,
        bundle_hash: attempt.bundle_hash.clone(),
        candidate_dir: attempt.candidate_dir.clone(),
        manifest_path: path.canonicalize()?,
        manifest_hash: sha256_file(&path)?,
    })
}

pub fn verify_candidate_evidence(
    manifest_path: impl AsRef<Path>,
    manifest_hash: &str,
    expected_plan_id: &str,
    expected_bundle_hash: &str,
) -> Result<VerifiedCandidate, VerificationError> {
    validate_plan_id(expected_plan_id)?;
    if !valid_sha256_tag(manifest_hash) || !valid_sha256_tag(expected_bundle_hash) {
        return Err(VerificationError::Evidence(
            "candidate verification hashes are invalid".to_string(),
        ));
    }
    let path = manifest_path.as_ref().canonicalize()?;
    if path.file_name().and_then(|name| name.to_str()) != Some("verification-final.json")
        || sha256_file(&path)? != manifest_hash
    {
        return Err(VerificationError::Evidence(
            "candidate final manifest does not recompute".to_string(),
        ));
    }
    let value = parse_exact_object(
        &fs::read_to_string(&path)?,
        &[
            "schema_version",
            "plan_id",
            "passed",
            "attempts",
            "bundle_hash",
            "candidate_dir",
            "attempt_manifest",
            "attempt_manifest_hash",
        ],
        "final manifest",
    )?;
    if number(&value, "schema_version")? != u64::from(VERIFICATION_SCHEMA_VERSION)
        || string(&value, "plan_id")? != expected_plan_id
        || !matches!(value.get("passed"), Some(JsonValue::Bool(true)))
        || string(&value, "bundle_hash")? != expected_bundle_hash
    {
        return Err(VerificationError::Evidence(
            "candidate final manifest does not match the provider claim".to_string(),
        ));
    }
    let attempts = u32::try_from(number(&value, "attempts")?).map_err(|_| {
        VerificationError::Evidence("candidate attempt count is invalid".to_string())
    })?;
    if attempts == 0 {
        return Err(VerificationError::Evidence(
            "candidate attempt count is empty".to_string(),
        ));
    }
    let attempt_path = PathBuf::from(string(&value, "attempt_manifest")?).canonicalize()?;
    let attempt_hash = string(&value, "attempt_manifest_hash")?;
    if !valid_sha256_tag(attempt_hash) || sha256_file(&attempt_path)? != attempt_hash {
        return Err(VerificationError::Evidence(
            "candidate attempt manifest does not recompute".to_string(),
        ));
    }
    let mut attempt = parse_attempt(&attempt_path)?;
    attempt.manifest_path = attempt_path;
    attempt.manifest_hash = attempt_hash.to_string();
    verify_attempt(&attempt)?;
    let candidate_dir = PathBuf::from(string(&value, "candidate_dir")?).canonicalize()?;
    if !attempt.passed
        || attempt.attempt != attempts
        || attempt.plan_id != expected_plan_id
        || attempt.bundle_hash != expected_bundle_hash
        || attempt.candidate_dir != candidate_dir
    {
        return Err(VerificationError::Evidence(
            "candidate attempt does not match final evidence".to_string(),
        ));
    }
    Ok(VerifiedCandidate {
        plan_id: expected_plan_id.to_string(),
        attempts,
        bundle_hash: expected_bundle_hash.to_string(),
        candidate_dir,
    })
}

fn verify_attempt(attempt: &VerificationAttempt) -> Result<(), VerificationError> {
    validate_plan_id(&attempt.plan_id)?;
    if attempt.attempt == 0 || !valid_sha256_tag(&attempt.bundle_hash) {
        return Err(VerificationError::Evidence(
            "candidate attempt identity is invalid".to_string(),
        ));
    }
    let candidate_dir = attempt.candidate_dir.canonicalize()?;
    for file in &attempt.files {
        verify_file(&candidate_dir, file)?;
    }
    let expected = ["cargo-check", "cargo-test", "cargo-clippy"];
    if attempt.passed
        && (attempt.commands.len() != expected.len()
            || attempt
                .commands
                .iter()
                .zip(expected)
                .any(|(command, name)| command.name != name || !command.passed()))
    {
        return Err(VerificationError::Evidence(
            "passing candidate did not pass every strict command".to_string(),
        ));
    }
    for command in &attempt.commands {
        verify_command(command)?;
    }
    Ok(())
}

fn verify_file(root: &Path, file: &FileEvidence) -> Result<(), VerificationError> {
    crate::model::validate_relative_path(&file.path)?;
    let path = root.join(&file.path).canonicalize()?;
    let metadata = fs::metadata(&path)?;
    if !path.starts_with(root)
        || metadata.len() != file.len as u64
        || !valid_sha256_tag(&file.hash)
        || sha256_file(&path)? != file.hash
    {
        return Err(VerificationError::Evidence(format!(
            "candidate file evidence does not recompute: {}",
            file.path
        )));
    }
    Ok(())
}

fn verify_command(command: &CommandEvidence) -> Result<(), VerificationError> {
    if command.name.is_empty()
        || command.program.is_empty()
        || !valid_sha256_tag(&command.stdout_hash)
        || !valid_sha256_tag(&command.stderr_hash)
    {
        return Err(VerificationError::Evidence(
            "candidate command evidence fields are invalid".to_string(),
        ));
    }
    let stdout = command.stdout_path.canonicalize()?;
    let stderr = command.stderr_path.canonicalize()?;
    if fs::metadata(&stdout)?.len() != command.stdout_len as u64
        || fs::metadata(&stderr)?.len() != command.stderr_len as u64
        || sha256_file(&stdout)? != command.stdout_hash
        || sha256_file(&stderr)? != command.stderr_hash
    {
        return Err(VerificationError::Evidence(format!(
            "candidate command logs do not recompute: {}",
            command.name
        )));
    }
    Ok(())
}

fn parse_attempt(path: &Path) -> Result<VerificationAttempt, VerificationError> {
    let value = parse_exact_object(
        &fs::read_to_string(path)?,
        &[
            "schema_version",
            "plan_id",
            "attempt",
            "passed",
            "candidate_dir",
            "bundle_hash",
            "files",
            "commands",
            "failure",
        ],
        "attempt manifest",
    )?;
    if number(&value, "schema_version")? != u64::from(VERIFICATION_SCHEMA_VERSION) {
        return Err(VerificationError::Evidence(
            "candidate attempt schema is invalid".to_string(),
        ));
    }
    let files = array(&value, "files")?
        .iter()
        .map(parse_file)
        .collect::<Result<Vec<_>, _>>()?;
    let commands = array(&value, "commands")?
        .iter()
        .map(parse_command)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VerificationAttempt {
        plan_id: string(&value, "plan_id")?.to_string(),
        attempt: u32::try_from(number(&value, "attempt")?).map_err(|_| {
            VerificationError::Evidence("candidate attempt number is invalid".to_string())
        })?,
        passed: boolean(&value, "passed")?,
        candidate_dir: PathBuf::from(string(&value, "candidate_dir")?),
        bundle_hash: string(&value, "bundle_hash")?.to_string(),
        files,
        commands,
        failure: string(&value, "failure")?.to_string(),
        manifest_path: path.to_path_buf(),
        manifest_hash: String::new(),
    })
}

fn parse_file(value: &JsonValue) -> Result<FileEvidence, VerificationError> {
    let value = exact_value_object(
        value,
        &["path", "hash", "len", "controller_owned"],
        "file evidence",
    )?;
    Ok(FileEvidence {
        path: string(value, "path")?.to_string(),
        hash: string(value, "hash")?.to_string(),
        len: usize::try_from(number(value, "len")?).map_err(|_| {
            VerificationError::Evidence("candidate file length is invalid".to_string())
        })?,
        controller_owned: boolean(value, "controller_owned")?,
    })
}

fn parse_command(value: &JsonValue) -> Result<CommandEvidence, VerificationError> {
    let value = exact_value_object(
        value,
        &[
            "name",
            "program",
            "args",
            "exit_code",
            "timed_out",
            "stdout_path",
            "stdout_hash",
            "stdout_len",
            "stderr_path",
            "stderr_hash",
            "stderr_len",
        ],
        "command evidence",
    )?;
    Ok(CommandEvidence {
        name: string(value, "name")?.to_string(),
        program: string(value, "program")?.to_string(),
        args: string_array(value, "args")?,
        exit_code: i32::try_from(signed_number(value, "exit_code")?).map_err(|_| {
            VerificationError::Evidence("candidate exit code is invalid".to_string())
        })?,
        timed_out: boolean(value, "timed_out")?,
        stdout_path: PathBuf::from(string(value, "stdout_path")?),
        stdout_hash: string(value, "stdout_hash")?.to_string(),
        stdout_len: usize::try_from(number(value, "stdout_len")?).map_err(|_| {
            VerificationError::Evidence("candidate stdout length is invalid".to_string())
        })?,
        stderr_path: PathBuf::from(string(value, "stderr_path")?),
        stderr_hash: string(value, "stderr_hash")?.to_string(),
        stderr_len: usize::try_from(number(value, "stderr_len")?).map_err(|_| {
            VerificationError::Evidence("candidate stderr length is invalid".to_string())
        })?,
    })
}

fn file_json(file: &FileEvidence) -> String {
    format!(
        "{{\"path\":\"{}\",\"hash\":\"{}\",\"len\":{},\"controller_owned\":{}}}",
        escape(&file.path),
        escape(&file.hash),
        file.len,
        file.controller_owned
    )
}

fn command_json(command: &CommandEvidence) -> String {
    let args = command
        .args
        .iter()
        .map(|arg| format!("\"{}\"", escape(arg)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"name\":\"{}\",\"program\":\"{}\",\"args\":[{}],\"exit_code\":{},\"timed_out\":{},\"stdout_path\":\"{}\",\"stdout_hash\":\"{}\",\"stdout_len\":{},\"stderr_path\":\"{}\",\"stderr_hash\":\"{}\",\"stderr_len\":{}}}",
        escape(&command.name),
        escape(&command.program),
        args,
        command.exit_code,
        command.timed_out,
        escape(&command.stdout_path.to_string_lossy()),
        escape(&command.stdout_hash),
        command.stdout_len,
        escape(&command.stderr_path.to_string_lossy()),
        escape(&command.stderr_hash),
        command.stderr_len
    )
}

fn write_immutable_verified(path: &Path, bytes: &[u8]) -> Result<(), VerificationError> {
    if path.exists() {
        if fs::read(path)? == bytes {
            return Ok(());
        }
        return Err(VerificationError::Evidence(format!(
            "immutable verification evidence conflict at {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| VerificationError::Io("verification path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("verification"),
        std::process::id(),
        now_ms()
    ));
    let result = (|| -> Result<(), VerificationError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let mut readback = Vec::new();
        File::open(&temp)?.read_to_end(&mut readback)?;
        if readback != bytes {
            return Err(VerificationError::Evidence(
                "verification evidence readback mismatch".to_string(),
            ));
        }
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn parse_exact_object(
    text: &str,
    fields: &[&str],
    label: &str,
) -> Result<JsonValue, VerificationError> {
    let value = parse_json(text)
        .map_err(|error| VerificationError::Evidence(format!("{label} JSON: {error}")))?;
    exact_value_object(&value, fields, label)?;
    Ok(value)
}

fn exact_value_object<'a>(
    value: &'a JsonValue,
    fields: &[&str],
    label: &str,
) -> Result<&'a JsonValue, VerificationError> {
    let object = value
        .as_object()
        .ok_or_else(|| VerificationError::Evidence(format!("{label} is not an object")))?;
    let expected: HashSet<&str> = fields.iter().copied().collect();
    let actual: HashSet<&str> = object.iter().map(|(name, _)| name.as_str()).collect();
    if actual != expected {
        return Err(VerificationError::Evidence(format!(
            "{label} fields are invalid"
        )));
    }
    Ok(value)
}

fn string<'a>(value: &'a JsonValue, key: &str) -> Result<&'a str, VerificationError> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| VerificationError::Evidence(format!("{key} is not a string")))
}

fn number(value: &JsonValue, key: &str) -> Result<u64, VerificationError> {
    value
        .get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| VerificationError::Evidence(format!("{key} is not an integer")))
}

fn signed_number(value: &JsonValue, key: &str) -> Result<i64, VerificationError> {
    let item = value
        .get(key)
        .ok_or_else(|| VerificationError::Evidence(format!("{key} is missing")))?;
    match item {
        JsonValue::Number(number) => number
            .parse::<i64>()
            .map_err(|_| VerificationError::Evidence(format!("{key} is not a signed integer"))),
        _ => Err(VerificationError::Evidence(format!(
            "{key} is not a signed integer"
        ))),
    }
}

fn boolean(value: &JsonValue, key: &str) -> Result<bool, VerificationError> {
    match value.get(key) {
        Some(JsonValue::Bool(value)) => Ok(*value),
        _ => Err(VerificationError::Evidence(format!(
            "{key} is not a boolean"
        ))),
    }
}

fn array<'a>(value: &'a JsonValue, key: &str) -> Result<&'a [JsonValue], VerificationError> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| VerificationError::Evidence(format!("{key} is not an array")))
}

fn string_array(value: &JsonValue, key: &str) -> Result<Vec<String>, VerificationError> {
    array(value, key)?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| VerificationError::Evidence(format!("{key} entry is not a string")))
        })
        .collect()
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
