use crate::model::{
    bundle_hash, candidate_profile, controller_json_harness, controller_manifest,
    json_validation_failure, validate_files, validate_plan_id, CandidateFile, CandidateProfile,
    CommandEvidence, FileEvidence, RepairEvidence, VerificationAttempt, VerificationError,
    VerificationSuccess, VerifiedCandidate, VERIFICATION_SCHEMA_VERSION,
};
use math_atoms_hash::{sha256_file, sha256_tagged, valid_sha256_tag};
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

pub(crate) fn store_repair(
    root: &Path,
    failed: &VerificationAttempt,
    model: &str,
    files: &[CandidateFile],
) -> Result<RepairEvidence, VerificationError> {
    verify_attempt(failed)?;
    if failed.passed {
        return Err(VerificationError::Evidence(
            "a passing candidate cannot be repaired".to_string(),
        ));
    }
    validate_model(model)?;
    validate_files(files)?;
    let expected_attempt = verification_attempt_dir(root, &failed.plan_id, failed.attempt);
    let expected_manifest = expected_attempt.join("attempt.json").canonicalize()?;
    if failed.manifest_path.canonicalize()? != expected_manifest {
        return Err(VerificationError::Evidence(
            "repair source attempt is outside this verifier".to_string(),
        ));
    }

    let repair_dir = expected_attempt.join("repair");
    let files_dir = repair_dir.join("files");
    for file in files {
        let path = files_dir.join(file.path.replace('\\', "/"));
        write_immutable_verified(&path, file.content.as_bytes())?;
    }
    let repaired_bundle_hash = bundle_hash(files)?;
    let file_entries = files
        .iter()
        .map(|file| repair_file_json(&files_dir, file))
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let text = format!(
        "{{\"schema_version\":{},\"plan_id\":\"{}\",\"after_attempt\":{},\"source_bundle_hash\":\"{}\",\"repaired_bundle_hash\":\"{}\",\"model\":\"{}\",\"files\":[{}]}}",
        VERIFICATION_SCHEMA_VERSION,
        escape(&failed.plan_id),
        failed.attempt,
        escape(&failed.bundle_hash),
        escape(&repaired_bundle_hash),
        escape(model),
        file_entries
    );
    let path = repair_dir.join("repair.json");
    write_immutable_verified(&path, text.as_bytes())?;
    let repair = RepairEvidence {
        plan_id: failed.plan_id.clone(),
        after_attempt: failed.attempt,
        source_bundle_hash: failed.bundle_hash.clone(),
        repaired_bundle_hash,
        model: model.to_string(),
        files: files.to_vec(),
        manifest_path: path.canonicalize()?,
        manifest_hash: sha256_file(&path)?,
    };
    verify_repair(&repair)?;
    Ok(repair)
}

pub(crate) fn load_repair(
    root: &Path,
    plan_id: &str,
    after_attempt: u32,
) -> Result<Option<RepairEvidence>, VerificationError> {
    validate_plan_id(plan_id)?;
    if after_attempt == 0 {
        return Err(VerificationError::Evidence(
            "repair attempt number is invalid".to_string(),
        ));
    }
    let path = verification_attempt_dir(root, plan_id, after_attempt)
        .join("repair")
        .join("repair.json");
    if !path.exists() {
        return Ok(None);
    }
    let repair = parse_repair(&path)?;
    verify_repair(&repair)?;
    Ok(Some(repair))
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
    let (attempts, repairs, history_hash) = load_chain(root, &attempt.plan_id, attempt.attempt)?;
    let final_attempt = attempts.last().ok_or_else(|| {
        VerificationError::Evidence("candidate verification chain is empty".to_string())
    })?;
    if final_attempt != attempt {
        return Err(VerificationError::Evidence(
            "final verification attempt differs from durable evidence".to_string(),
        ));
    }
    let dir = root.join(&attempt.plan_id).join("candidate-verification");
    fs::create_dir_all(&dir)?;
    let path = dir.join("verification-final.json");
    let text = format!(
        "{{\"schema_version\":{},\"plan_id\":\"{}\",\"passed\":true,\"attempts\":{},\"repairs\":{},\"bundle_hash\":\"{}\",\"history_hash\":\"{}\",\"candidate_dir\":\"{}\",\"attempt_manifest\":\"{}\",\"attempt_manifest_hash\":\"{}\"}}",
        VERIFICATION_SCHEMA_VERSION,
        escape(&attempt.plan_id),
        attempt.attempt,
        repairs.len(),
        escape(&attempt.bundle_hash),
        escape(&history_hash),
        escape(&attempt.candidate_dir.to_string_lossy()),
        escape(&attempt.manifest_path.to_string_lossy()),
        escape(&attempt.manifest_hash)
    );
    write_immutable_verified(&path, text.as_bytes())?;
    Ok(VerificationSuccess {
        plan_id: attempt.plan_id.clone(),
        attempts: attempt.attempt,
        repairs: repairs.len() as u32,
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
            "repairs",
            "bundle_hash",
            "history_hash",
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
    if attempts == 0 || attempts > crate::MAX_VERIFICATION_ATTEMPTS {
        return Err(VerificationError::Evidence(
            "candidate attempt count is invalid".to_string(),
        ));
    }
    let repairs = u32::try_from(number(&value, "repairs")?).map_err(|_| {
        VerificationError::Evidence("candidate repair count is invalid".to_string())
    })?;
    if repairs != attempts - 1 {
        return Err(VerificationError::Evidence(
            "candidate repair count does not close the attempt chain".to_string(),
        ));
    }
    let claimed_history_hash = string(&value, "history_hash")?;
    if !valid_sha256_tag(claimed_history_hash) {
        return Err(VerificationError::Evidence(
            "candidate history hash is invalid".to_string(),
        ));
    }
    let verification_dir = path.parent().ok_or_else(|| {
        VerificationError::Evidence("candidate final manifest has no parent".to_string())
    })?;
    if verification_dir.file_name().and_then(|name| name.to_str()) != Some("candidate-verification")
    {
        return Err(VerificationError::Evidence(
            "candidate final manifest is outside its verification directory".to_string(),
        ));
    }
    let plan_dir = verification_dir.parent().ok_or_else(|| {
        VerificationError::Evidence("candidate verification directory has no plan".to_string())
    })?;
    if plan_dir.file_name().and_then(|name| name.to_str()) != Some(expected_plan_id) {
        return Err(VerificationError::Evidence(
            "candidate verification directory does not match its plan".to_string(),
        ));
    }
    let root = plan_dir.parent().ok_or_else(|| {
        VerificationError::Evidence("candidate verification directory has no root".to_string())
    })?;
    let (chain_attempts, chain_repairs, recomputed_history_hash) =
        load_chain(root, expected_plan_id, attempts)?;
    if chain_repairs.len() as u32 != repairs || recomputed_history_hash != claimed_history_hash {
        return Err(VerificationError::Evidence(
            "candidate verification history does not recompute".to_string(),
        ));
    }
    let attempt = chain_attempts.last().ok_or_else(|| {
        VerificationError::Evidence("candidate verification chain is empty".to_string())
    })?;
    let attempt_path = PathBuf::from(string(&value, "attempt_manifest")?).canonicalize()?;
    let attempt_hash = string(&value, "attempt_manifest_hash")?;
    if !valid_sha256_tag(attempt_hash)
        || sha256_file(&attempt_path)? != attempt_hash
        || attempt_path != attempt.manifest_path
        || attempt_hash != attempt.manifest_hash
    {
        return Err(VerificationError::Evidence(
            "candidate attempt manifest does not recompute".to_string(),
        ));
    }
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
        repairs,
        bundle_hash: expected_bundle_hash.to_string(),
        candidate_dir,
    })
}

fn load_chain(
    root: &Path,
    plan_id: &str,
    final_attempt: u32,
) -> Result<(Vec<VerificationAttempt>, Vec<RepairEvidence>, String), VerificationError> {
    validate_plan_id(plan_id)?;
    if final_attempt == 0 || final_attempt > crate::MAX_VERIFICATION_ATTEMPTS {
        return Err(VerificationError::Evidence(
            "candidate verification chain length is invalid".to_string(),
        ));
    }
    let mut attempts = Vec::with_capacity(final_attempt as usize);
    let mut repairs = Vec::with_capacity(final_attempt.saturating_sub(1) as usize);
    let mut history = format!("schema:{}\nplan:{}\n", VERIFICATION_SCHEMA_VERSION, plan_id);
    let mut prior_repair: Option<RepairEvidence> = None;
    for ordinal in 1..=final_attempt {
        let attempt = load_attempt(root, plan_id, ordinal)?.ok_or_else(|| {
            VerificationError::Evidence(format!(
                "candidate verification attempt {ordinal} is missing"
            ))
        })?;
        if attempt.plan_id != plan_id || attempt.attempt != ordinal {
            return Err(VerificationError::Evidence(format!(
                "candidate verification attempt {ordinal} has the wrong identity"
            )));
        }
        if ordinal < final_attempt && attempt.passed {
            return Err(VerificationError::Evidence(format!(
                "candidate verification attempt {ordinal} passed before the claimed final attempt"
            )));
        }
        if ordinal == final_attempt && !attempt.passed {
            return Err(VerificationError::Evidence(
                "candidate verification chain does not end in a pass".to_string(),
            ));
        }
        if let Some(repair) = prior_repair.take() {
            if repair.repaired_bundle_hash != attempt.bundle_hash {
                return Err(VerificationError::Evidence(format!(
                    "repair after attempt {} does not bind attempt {ordinal}",
                    ordinal - 1
                )));
            }
        }
        history.push_str(&format!(
            "attempt:{ordinal}:{}:{}\n",
            attempt.bundle_hash, attempt.manifest_hash
        ));
        attempts.push(attempt.clone());

        if ordinal < final_attempt {
            let repair = load_repair(root, plan_id, ordinal)?.ok_or_else(|| {
                VerificationError::Evidence(format!(
                    "candidate repair after attempt {ordinal} is missing"
                ))
            })?;
            if repair.source_bundle_hash != attempt.bundle_hash
                || repair.after_attempt != ordinal
                || repair.plan_id != plan_id
            {
                return Err(VerificationError::Evidence(format!(
                    "candidate repair after attempt {ordinal} is not bound to its failure"
                )));
            }
            history.push_str(&format!(
                "repair:{ordinal}:{}:{}:{}\n",
                repair.source_bundle_hash, repair.repaired_bundle_hash, repair.manifest_hash
            ));
            prior_repair = Some(repair.clone());
            repairs.push(repair);
        }
    }
    if load_repair(root, plan_id, final_attempt)?.is_some() {
        return Err(VerificationError::Evidence(
            "passing candidate has a trailing repair".to_string(),
        ));
    }
    if final_attempt < crate::MAX_VERIFICATION_ATTEMPTS
        && load_attempt(root, plan_id, final_attempt + 1)?.is_some()
    {
        return Err(VerificationError::Evidence(
            "candidate verification has an attempt after its claimed final pass".to_string(),
        ));
    }
    Ok((attempts, repairs, sha256_tagged(history.as_bytes())))
}

fn parse_repair(path: &Path) -> Result<RepairEvidence, VerificationError> {
    let path = path.canonicalize()?;
    if path.file_name().and_then(|name| name.to_str()) != Some("repair.json") {
        return Err(VerificationError::Evidence(
            "candidate repair manifest name is invalid".to_string(),
        ));
    }
    let value = parse_exact_object(
        &fs::read_to_string(&path)?,
        &[
            "schema_version",
            "plan_id",
            "after_attempt",
            "source_bundle_hash",
            "repaired_bundle_hash",
            "model",
            "files",
        ],
        "repair manifest",
    )?;
    if number(&value, "schema_version")? != u64::from(VERIFICATION_SCHEMA_VERSION) {
        return Err(VerificationError::Evidence(
            "candidate repair schema is invalid".to_string(),
        ));
    }
    let repair_dir = path.parent().ok_or_else(|| {
        VerificationError::Evidence("candidate repair manifest has no parent".to_string())
    })?;
    let files_dir = repair_dir.join("files").canonicalize()?;
    let mut files = Vec::new();
    for item in array(&value, "files")? {
        let item = exact_value_object(item, &["path", "hash", "len"], "repair file")?;
        let relative = string(item, "path")?;
        crate::model::validate_relative_path(relative)?;
        let claimed_hash = string(item, "hash")?;
        let claimed_len = usize::try_from(number(item, "len")?).map_err(|_| {
            VerificationError::Evidence("candidate repair file length is invalid".to_string())
        })?;
        let file_path = files_dir.join(relative).canonicalize()?;
        let metadata = fs::metadata(&file_path)?;
        if !file_path.starts_with(&files_dir)
            || !valid_sha256_tag(claimed_hash)
            || metadata.len() != claimed_len as u64
            || sha256_file(&file_path)? != claimed_hash
        {
            return Err(VerificationError::Evidence(format!(
                "candidate repair file does not recompute: {relative}"
            )));
        }
        files.push(CandidateFile::new(
            relative,
            fs::read_to_string(file_path)?,
        )?);
    }
    let repair = RepairEvidence {
        plan_id: string(&value, "plan_id")?.to_string(),
        after_attempt: u32::try_from(number(&value, "after_attempt")?).map_err(|_| {
            VerificationError::Evidence("candidate repair attempt is invalid".to_string())
        })?,
        source_bundle_hash: string(&value, "source_bundle_hash")?.to_string(),
        repaired_bundle_hash: string(&value, "repaired_bundle_hash")?.to_string(),
        model: string(&value, "model")?.to_string(),
        files,
        manifest_hash: sha256_file(&path)?,
        manifest_path: path,
    };
    validate_repair_fields(&repair)?;
    Ok(repair)
}

fn verify_repair(repair: &RepairEvidence) -> Result<(), VerificationError> {
    validate_repair_fields(repair)?;
    if !valid_sha256_tag(&repair.manifest_hash)
        || sha256_file(&repair.manifest_path)? != repair.manifest_hash
    {
        return Err(VerificationError::Evidence(
            "candidate repair manifest does not recompute".to_string(),
        ));
    }
    let parsed = parse_repair(&repair.manifest_path)?;
    if parsed != *repair {
        return Err(VerificationError::Evidence(
            "candidate repair differs from its durable manifest".to_string(),
        ));
    }
    Ok(())
}

fn validate_repair_fields(repair: &RepairEvidence) -> Result<(), VerificationError> {
    validate_plan_id(&repair.plan_id)?;
    validate_model(&repair.model)?;
    validate_files(&repair.files)?;
    if repair.after_attempt == 0
        || repair.after_attempt >= crate::MAX_VERIFICATION_ATTEMPTS
        || !valid_sha256_tag(&repair.source_bundle_hash)
        || !valid_sha256_tag(&repair.repaired_bundle_hash)
        || repair.source_bundle_hash == repair.repaired_bundle_hash
        || bundle_hash(&repair.files)? != repair.repaired_bundle_hash
    {
        return Err(VerificationError::Evidence(
            "candidate repair identity is invalid".to_string(),
        ));
    }
    Ok(())
}

fn validate_model(model: &str) -> Result<(), VerificationError> {
    if model.trim() != model
        || model.is_empty()
        || model.len() > 240
        || model.chars().any(char::is_control)
    {
        return Err(VerificationError::Evidence(
            "candidate repair model is invalid".to_string(),
        ));
    }
    Ok(())
}

fn verify_attempt(attempt: &VerificationAttempt) -> Result<(), VerificationError> {
    validate_plan_id(&attempt.plan_id)?;
    if attempt.attempt == 0
        || attempt.attempt > crate::MAX_VERIFICATION_ATTEMPTS
        || !valid_sha256_tag(&attempt.bundle_hash)
        || !valid_sha256_tag(&attempt.manifest_hash)
        || sha256_file(&attempt.manifest_path)? != attempt.manifest_hash
    {
        return Err(VerificationError::Evidence(
            "candidate attempt identity is invalid".to_string(),
        ));
    }
    let manifest_path = attempt.manifest_path.canonicalize()?;
    let candidate_dir = attempt.candidate_dir.canonicalize()?;
    let attempt_dir = manifest_path.parent().ok_or_else(|| {
        VerificationError::Evidence("candidate attempt manifest has no parent".to_string())
    })?;
    if manifest_path.file_name().and_then(|name| name.to_str()) != Some("attempt.json")
        || candidate_dir.parent() != Some(attempt_dir)
        || candidate_dir.file_name().and_then(|name| name.to_str()) != Some("candidate")
        || attempt_dir.file_name().and_then(|name| name.to_str())
            != Some(format!("attempt-{:03}", attempt.attempt).as_str())
    {
        return Err(VerificationError::Evidence(
            "candidate attempt paths are invalid".to_string(),
        ));
    }
    let mut candidate_files = Vec::new();
    let mut controller_files = Vec::new();
    for file in &attempt.files {
        verify_file(&candidate_dir, file)?;
        if file.controller_owned {
            controller_files.push(CandidateFile::new(
                &file.path,
                fs::read_to_string(candidate_dir.join(&file.path))?,
            )?);
        } else {
            candidate_files.push(CandidateFile::new(
                &file.path,
                fs::read_to_string(candidate_dir.join(&file.path))?,
            )?);
        }
    }
    let profile = candidate_profile(&candidate_files)?;
    let expected_controller_files = expected_controller_files(profile, &candidate_files)?;
    if controller_files != expected_controller_files
        || bundle_hash(&candidate_files)? != attempt.bundle_hash
    {
        return Err(VerificationError::Evidence(
            "candidate bundle does not recompute from attempt files".to_string(),
        ));
    }
    if attempt.passed && json_validation_failure(&candidate_files)?.is_some() {
        return Err(VerificationError::Evidence(
            "passing JSON candidate does not parse".to_string(),
        ));
    }
    let expected = ["cargo-check", "cargo-test", "cargo-clippy"];
    if attempt.commands.is_empty()
        || attempt.commands.len() > expected.len()
        || attempt
            .commands
            .iter()
            .zip(expected)
            .any(|(command, name)| command.name != name)
    {
        return Err(VerificationError::Evidence(
            "candidate strict command sequence is invalid".to_string(),
        ));
    }
    for (index, command) in attempt.commands.iter().enumerate() {
        verify_command(command, attempt_dir, index)?;
    }
    if attempt.passed {
        if attempt.commands.len() != expected.len()
            || attempt.commands.iter().any(|command| !command.passed())
            || !attempt.failure.is_empty()
        {
            return Err(VerificationError::Evidence(
                "passing candidate did not pass every strict command".to_string(),
            ));
        }
    } else if attempt.failure.is_empty()
        || attempt.commands.last().is_none_or(CommandEvidence::passed)
        || attempt.commands[..attempt.commands.len() - 1]
            .iter()
            .any(|command| !command.passed())
    {
        return Err(VerificationError::Evidence(
            "failed candidate does not contain a closed strict-command failure".to_string(),
        ));
    }
    Ok(())
}

fn expected_controller_files(
    profile: CandidateProfile,
    files: &[CandidateFile],
) -> Result<Vec<CandidateFile>, VerificationError> {
    if files.iter().any(|file| {
        file.path
            .replace('\\', "/")
            .eq_ignore_ascii_case("Cargo.toml")
    }) {
        return Ok(Vec::new());
    }
    if profile == CandidateProfile::Json {
        return Ok(vec![
            CandidateFile::new("Cargo.toml", controller_manifest(""))?,
            CandidateFile::new("src/lib.rs", controller_json_harness(files)?)?,
        ]);
    }
    let rust_files = files
        .iter()
        .filter(|file| file.path.to_ascii_lowercase().ends_with(".rs"))
        .collect::<Vec<_>>();
    let has_standard_target = files.iter().any(|file| {
        matches!(
            file.path.replace('\\', "/").as_str(),
            "src/main.rs" | "src/lib.rs"
        )
    });
    let extra = if has_standard_target {
        String::new()
    } else if rust_files.len() == 1 {
        format!(
            "\n[[bin]]\nname = \"candidate\"\npath = \"{}\"\n",
            toml_escape(&rust_files[0].path.replace('\\', "/"))
        )
    } else {
        return Err(VerificationError::UnsupportedCandidate(
            "candidate must provide Cargo.toml, src/main.rs, src/lib.rs, or one Rust source file"
                .to_string(),
        ));
    };
    Ok(vec![CandidateFile::new(
        "Cargo.toml",
        controller_manifest(&extra),
    )?])
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

fn verify_command(
    command: &CommandEvidence,
    attempt_dir: &Path,
    index: usize,
) -> Result<(), VerificationError> {
    let expected_args: [&[&str]; 3] = [
        &["check", "--all-targets", "--offline"],
        &["test", "--all-targets", "--offline"],
        &[
            "clippy",
            "--all-targets",
            "--offline",
            "--",
            "-D",
            "warnings",
        ],
    ];
    let expected_args = expected_args.get(index).ok_or_else(|| {
        VerificationError::Evidence("candidate command ordinal is invalid".to_string())
    })?;
    let expected_args = expected_args
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    let program_name = Path::new(&command.program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if command.name.is_empty()
        || command.program.is_empty()
        || !matches!(program_name.as_str(), "cargo" | "cargo.exe")
        || command.args != expected_args
        || !valid_sha256_tag(&command.stdout_hash)
        || !valid_sha256_tag(&command.stderr_hash)
    {
        return Err(VerificationError::Evidence(
            "candidate command evidence fields are invalid".to_string(),
        ));
    }
    let stdout = command.stdout_path.canonicalize()?;
    let stderr = command.stderr_path.canonicalize()?;
    if !stdout.starts_with(attempt_dir)
        || !stderr.starts_with(attempt_dir)
        || fs::metadata(&stdout)?.len() != command.stdout_len as u64
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

fn repair_file_json(root: &Path, file: &CandidateFile) -> Result<String, VerificationError> {
    let path = root.join(file.path.replace('\\', "/")).canonicalize()?;
    if !path.starts_with(root.canonicalize()?) {
        return Err(VerificationError::Evidence(
            "candidate repair file escaped its root".to_string(),
        ));
    }
    let metadata = fs::metadata(&path)?;
    Ok(format!(
        "{{\"path\":\"{}\",\"hash\":\"{}\",\"len\":{}}}",
        escape(&file.path.replace('\\', "/")),
        escape(&sha256_file(&path)?),
        metadata.len()
    ))
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

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
