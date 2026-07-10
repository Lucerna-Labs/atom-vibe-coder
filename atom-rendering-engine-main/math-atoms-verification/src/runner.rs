use crate::model::{
    bundle_hash, candidate_profile, clean_failure, controller_json_harness, controller_manifest,
    validate_files, validate_plan_id, CandidateFile, CandidateProfile, CommandEvidence,
    FileEvidence, RepairEvidence, VerificationAttempt, VerificationError, VerificationPolicy,
    VerificationSuccess, MAX_LOG_BYTES, MAX_VERIFICATION_ATTEMPTS,
};
use crate::store::{
    finalize_success, load_attempt, load_repair, store_repair, verification_attempt_dir,
    write_attempt_manifest,
};
use math_atoms_hash::sha256_file;
use math_atoms_lock::acquire_file_lease;
use math_atoms_secrets::redact_sensitive_text;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const LOCK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateVerifier {
    root: PathBuf,
    policy: VerificationPolicy,
}

impl CandidateVerifier {
    pub fn new(root: impl Into<PathBuf>, policy: VerificationPolicy) -> Self {
        Self {
            root: root.into(),
            policy,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn verify_attempt(
        &self,
        plan_id: &str,
        attempt: u32,
        files: &[CandidateFile],
    ) -> Result<VerificationAttempt, VerificationError> {
        validate_plan_id(plan_id)?;
        validate_files(files)?;
        if attempt == 0 || attempt > MAX_VERIFICATION_ATTEMPTS {
            return Err(VerificationError::InvalidCandidate(format!(
                "verification attempt must be between 1 and {MAX_VERIFICATION_ATTEMPTS}"
            )));
        }
        fs::create_dir_all(&self.root)?;
        let lock = self.root.join(format!("{plan_id}.verification.lock"));
        let _lease = acquire_file_lease(&lock, LOCK_TIMEOUT, STALE_LOCK_AGE)?;
        if let Some(existing) = load_attempt(&self.root, plan_id, attempt)? {
            if existing.bundle_hash != bundle_hash(files)? {
                return Err(VerificationError::Evidence(format!(
                    "attempt {attempt} already exists for a different candidate"
                )));
            }
            return Ok(existing);
        }

        let attempt_dir = verification_attempt_dir(&self.root, plan_id, attempt);
        let candidate_dir = attempt_dir.join("candidate");
        fs::create_dir_all(&candidate_dir)?;
        let evidence = materialize_candidate(&candidate_dir, files)?;
        let normalized_files = normalized_files_with_controller_manifest(&candidate_dir, files)?;
        let expected_bundle_hash = bundle_hash(files)?;
        let cargo_target = attempt_dir.join("cargo-target");
        fs::create_dir_all(&cargo_target)?;

        let mut commands = Vec::new();
        let specs = strict_commands();
        for (index, spec) in specs.iter().enumerate() {
            let command = run_command(
                &attempt_dir,
                &candidate_dir,
                &cargo_target,
                index + 1,
                spec,
                self.policy.command_timeout_seconds,
            )?;
            let passed = command.passed();
            commands.push(command);
            if !passed {
                break;
            }
        }
        let passed = commands.len() == specs.len() && commands.iter().all(CommandEvidence::passed);
        let failure = if passed {
            String::new()
        } else {
            failure_summary(&commands)?
        };
        let mut result = VerificationAttempt {
            plan_id: plan_id.to_string(),
            attempt,
            passed,
            candidate_dir: candidate_dir.canonicalize()?,
            bundle_hash: expected_bundle_hash,
            files: evidence,
            commands,
            failure,
            manifest_path: PathBuf::new(),
            manifest_hash: String::new(),
        };
        let written = write_attempt_manifest(&attempt_dir, &result, &normalized_files)?;
        result.manifest_path = written.0;
        result.manifest_hash = written.1;
        Ok(result)
    }

    pub fn finalize(
        &self,
        attempt: &VerificationAttempt,
    ) -> Result<VerificationSuccess, VerificationError> {
        if !attempt.passed {
            return Err(VerificationError::Evidence(
                "a failed candidate cannot be finalized".to_string(),
            ));
        }
        finalize_success(&self.root, attempt)
    }

    pub fn load_repair(
        &self,
        plan_id: &str,
        after_attempt: u32,
    ) -> Result<Option<RepairEvidence>, VerificationError> {
        load_repair(&self.root, plan_id, after_attempt)
    }

    pub fn store_repair(
        &self,
        failed: &VerificationAttempt,
        model: &str,
        files: &[CandidateFile],
    ) -> Result<RepairEvidence, VerificationError> {
        if failed.passed {
            return Err(VerificationError::Evidence(
                "a passing candidate cannot have a repair".to_string(),
            ));
        }
        validate_files(files)?;
        store_repair(&self.root, failed, model, files)
    }
}

#[derive(Clone, Debug)]
struct CommandSpec {
    name: &'static str,
    args: &'static [&'static str],
}

fn strict_commands() -> [CommandSpec; 3] {
    [
        CommandSpec {
            name: "cargo-check",
            args: &["check", "--all-targets", "--offline"],
        },
        CommandSpec {
            name: "cargo-test",
            args: &["test", "--all-targets", "--offline"],
        },
        CommandSpec {
            name: "cargo-clippy",
            args: &[
                "clippy",
                "--all-targets",
                "--offline",
                "--",
                "-D",
                "warnings",
            ],
        },
    ]
}

fn materialize_candidate(
    candidate_dir: &Path,
    files: &[CandidateFile],
) -> Result<Vec<FileEvidence>, VerificationError> {
    let mut evidence = Vec::new();
    for item in files {
        let normalized = item.path.replace('\\', "/");
        if normalized == "build.rs" || normalized.starts_with(".cargo/") {
            return Err(VerificationError::UnsupportedCandidate(format!(
                "candidate control file is forbidden: {}",
                item.path
            )));
        }
        let path = candidate_dir.join(&normalized);
        write_immutable(&path, item.content.as_bytes())?;
        evidence.push(file_evidence(candidate_dir, &path, false)?);
    }
    ensure_manifest(
        candidate_dir,
        files,
        candidate_profile(files)?,
        &mut evidence,
    )?;
    let manifest = fs::read_to_string(candidate_dir.join("Cargo.toml"))?;
    validate_dependency_free_manifest(&manifest)?;
    Ok(evidence)
}

fn ensure_manifest(
    candidate_dir: &Path,
    files: &[CandidateFile],
    profile: CandidateProfile,
    evidence: &mut Vec<FileEvidence>,
) -> Result<(), VerificationError> {
    if files.iter().any(|item| {
        item.path
            .replace('\\', "/")
            .eq_ignore_ascii_case("Cargo.toml")
    }) {
        return Ok(());
    }
    if profile == CandidateProfile::Json {
        let manifest_path = candidate_dir.join("Cargo.toml");
        write_immutable(&manifest_path, controller_manifest("").as_bytes())?;
        evidence.push(file_evidence(candidate_dir, &manifest_path, true)?);
        let harness_path = candidate_dir.join("src/lib.rs");
        write_immutable(&harness_path, controller_json_harness(files)?.as_bytes())?;
        evidence.push(file_evidence(candidate_dir, &harness_path, true)?);
        return Ok(());
    }

    let rust_files: Vec<&CandidateFile> = files
        .iter()
        .filter(|item| item.path.to_ascii_lowercase().ends_with(".rs"))
        .collect();
    let has_standard_target = files.iter().any(|item| {
        matches!(
            item.path.replace('\\', "/").as_str(),
            "src/main.rs" | "src/lib.rs"
        )
    });
    let manifest = if has_standard_target {
        controller_manifest("")
    } else if rust_files.len() == 1 {
        controller_manifest(&format!(
            "\n[[bin]]\nname = \"candidate\"\npath = \"{}\"\n",
            toml_escape(&rust_files[0].path.replace('\\', "/"))
        ))
    } else {
        return Err(VerificationError::UnsupportedCandidate(
            "candidate must provide Cargo.toml, src/main.rs, src/lib.rs, or one Rust source file"
                .to_string(),
        ));
    };
    let path = candidate_dir.join("Cargo.toml");
    write_immutable(&path, manifest.as_bytes())?;
    evidence.push(file_evidence(candidate_dir, &path, true)?);
    Ok(())
}

fn validate_dependency_free_manifest(text: &str) -> Result<(), VerificationError> {
    let mut dependency_table = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let table = line
                .trim_matches(|ch| ch == '[' || ch == ']')
                .trim()
                .to_ascii_lowercase();
            dependency_table = table == "dependencies"
                || table == "dev-dependencies"
                || table == "build-dependencies"
                || table.ends_with(".dependencies")
                || table.ends_with(".dev-dependencies")
                || table.ends_with(".build-dependencies");
            continue;
        }
        let compact = line.replace(' ', "").to_ascii_lowercase();
        if dependency_table && line.contains('=') {
            return Err(VerificationError::UnsupportedCandidate(
                "generated candidates must remain dependency-free".to_string(),
            ));
        }
        if compact.starts_with("build=") {
            return Err(VerificationError::UnsupportedCandidate(
                "generated build scripts are forbidden".to_string(),
            ));
        }
    }
    Ok(())
}

fn normalized_files_with_controller_manifest(
    candidate_dir: &Path,
    files: &[CandidateFile],
) -> Result<Vec<CandidateFile>, VerificationError> {
    let mut normalized = files.to_vec();
    if !normalized.iter().any(|item| {
        item.path
            .replace('\\', "/")
            .eq_ignore_ascii_case("Cargo.toml")
    }) {
        normalized.push(CandidateFile::new(
            "Cargo.toml",
            fs::read_to_string(candidate_dir.join("Cargo.toml"))?,
        )?);
    }
    Ok(normalized)
}

fn run_command(
    attempt_dir: &Path,
    candidate_dir: &Path,
    cargo_target: &Path,
    ordinal: usize,
    spec: &CommandSpec,
    timeout_seconds: u64,
) -> Result<CommandEvidence, VerificationError> {
    let stdout_path = attempt_dir.join(format!("{ordinal:02}-{}.stdout.log", spec.name));
    let stderr_path = attempt_dir.join(format!("{ordinal:02}-{}.stderr.log", spec.name));
    let stdout = File::create(&stdout_path)?;
    let stderr = File::create(&stderr_path)?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(&cargo);
    command
        .args(spec.args)
        .current_dir(candidate_dir)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", cargo_target)
        .env("RUSTFLAGS", "-D warnings")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    remove_sensitive_environment(&mut command);
    let mut child = command.spawn().map_err(|error| {
        VerificationError::Command(format!("failed to start {}: {error}", spec.name))
    })?;
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let (exit_code, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status.code().unwrap_or(-1), false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait()?;
            break (status.code().unwrap_or(-1), true);
        }
        thread::sleep(Duration::from_millis(25));
    };
    let stdout_meta = fs::metadata(&stdout_path)?;
    let stderr_meta = fs::metadata(&stderr_path)?;
    if stdout_meta.len() > MAX_LOG_BYTES as u64 || stderr_meta.len() > MAX_LOG_BYTES as u64 {
        return Err(VerificationError::Command(format!(
            "{} output exceeded {} bytes",
            spec.name, MAX_LOG_BYTES
        )));
    }
    Ok(CommandEvidence {
        name: spec.name.to_string(),
        program: cargo,
        args: spec.args.iter().map(|value| (*value).to_string()).collect(),
        exit_code,
        timed_out,
        stdout_hash: sha256_file(&stdout_path)?,
        stdout_len: stdout_meta.len() as usize,
        stdout_path: stdout_path.canonicalize()?,
        stderr_hash: sha256_file(&stderr_path)?,
        stderr_len: stderr_meta.len() as usize,
        stderr_path: stderr_path.canonicalize()?,
    })
}

fn remove_sensitive_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"]
            .iter()
            .any(|needle| upper.contains(needle))
        {
            command.env_remove(name);
        }
    }
}

fn failure_summary(commands: &[CommandEvidence]) -> Result<String, VerificationError> {
    let Some(command) = commands.iter().find(|item| !item.passed()) else {
        return Ok(String::new());
    };
    let stdout = read_limited(&command.stdout_path, 8 * 1024)?;
    let stderr = read_limited(&command.stderr_path, 16 * 1024)?;
    Ok(clean_failure(&format!(
        "{} failed (exit={}, timed_out={})\nstdout:\n{}\nstderr:\n{}",
        command.name, command.exit_code, command.timed_out, stdout, stderr
    )))
}

fn read_limited(path: &Path, limit: usize) -> Result<String, VerificationError> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref().take(limit as u64).read_to_end(&mut bytes)?;
    Ok(redact_sensitive_text(&String::from_utf8_lossy(&bytes)))
}

fn file_evidence(
    root: &Path,
    path: &Path,
    controller_owned: bool,
) -> Result<FileEvidence, VerificationError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| VerificationError::Evidence("candidate file escaped its root".to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let metadata = fs::metadata(path)?;
    Ok(FileEvidence {
        path: relative,
        hash: sha256_file(path)?,
        len: metadata.len() as usize,
        controller_owned,
    })
}

fn write_immutable(path: &Path, bytes: &[u8]) -> Result<(), VerificationError> {
    if path.exists() {
        let existing = fs::read(path)?;
        if existing == bytes {
            return Ok(());
        }
        return Err(VerificationError::Evidence(format!(
            "immutable candidate conflict at {}",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    if fs::read(path)? != bytes {
        return Err(VerificationError::Evidence(format!(
            "candidate readback mismatch at {}",
            path.display()
        )));
    }
    Ok(())
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify_candidate_evidence;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "math-atoms-verification-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn crate_files(source: &str) -> Vec<CandidateFile> {
        vec![
            CandidateFile::new(
                "Cargo.toml",
                "[package]\nname=\"verified-fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n\n[workspace]\n",
            )
            .unwrap(),
            CandidateFile::new("src/lib.rs", source).unwrap(),
        ]
    }

    #[test]
    fn real_cargo_gate_passes_and_reverifies_immutable_evidence() {
        let root = temp_root("pass");
        let verifier = CandidateVerifier::new(&root, VerificationPolicy::strict(120).unwrap());
        let attempt = verifier
            .verify_attempt(
                "work-1234567890abcdef12345678",
                1,
                &crate_files(
                    "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)] mod tests { use super::*; #[test] fn adds() { assert_eq!(add(2, 3), 5); } }\n",
                ),
            )
            .unwrap();
        assert!(attempt.passed, "{}", attempt.failure);
        assert_eq!(attempt.commands.len(), 3);
        let success = verifier.finalize(&attempt).unwrap();
        let verified = verify_candidate_evidence(
            &success.manifest_path,
            &success.manifest_hash,
            &success.plan_id,
            &success.bundle_hash,
        )
        .unwrap();
        assert_eq!(verified.attempts, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn broken_candidate_produces_real_failure_evidence_and_cannot_finalize() {
        let root = temp_root("fail");
        let verifier = CandidateVerifier::new(&root, VerificationPolicy::strict(120).unwrap());
        let attempt = verifier
            .verify_attempt(
                "work-abcdefabcdefabcdefabcdef",
                1,
                &crate_files("pub fn broken() -> i32 { missing_name }\n"),
            )
            .unwrap();
        assert!(!attempt.passed);
        assert!(attempt.failure.contains("missing_name"));
        assert!(verifier.finalize(&attempt).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repair_chain_requires_a_fresh_pass_and_recomputes_every_transition() {
        let root = temp_root("repair-chain");
        let verifier = CandidateVerifier::new(&root, VerificationPolicy::strict(120).unwrap());
        let plan_id = "work-fedcbafedcbafedcbafedcba";
        let failed = verifier
            .verify_attempt(
                plan_id,
                1,
                &crate_files("pub fn answer() -> i32 { missing_name }\n"),
            )
            .unwrap();
        assert!(!failed.passed);

        let repaired_files = crate_files(
            "pub fn answer() -> i32 { 42 }\n#[cfg(test)] mod tests { use super::*; #[test] fn answer_is_42() { assert_eq!(answer(), 42); } }\n",
        );
        let repair = verifier
            .store_repair(&failed, "deepseek-chat", &repaired_files)
            .unwrap();
        assert_eq!(repair.after_attempt, 1);
        let passed = verifier.verify_attempt(plan_id, 2, &repair.files).unwrap();
        assert!(passed.passed, "{}", passed.failure);

        let success = verifier.finalize(&passed).unwrap();
        let verified = verify_candidate_evidence(
            &success.manifest_path,
            &success.manifest_hash,
            plan_id,
            &success.bundle_hash,
        )
        .unwrap();
        assert_eq!(verified.attempts, 2);
        assert_eq!(verified.repairs, 1);

        let repair_source = root
            .join(plan_id)
            .join("candidate-verification")
            .join("attempt-001")
            .join("repair")
            .join("files")
            .join("src")
            .join("lib.rs");
        fs::write(&repair_source, "pub fn answer() -> i32 { 7 }\n").unwrap();
        assert!(verify_candidate_evidence(
            &success.manifest_path,
            &success.manifest_hash,
            plan_id,
            &success.bundle_hash,
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dependency_and_build_script_candidates_fail_closed() {
        let root = temp_root("policy");
        let verifier = CandidateVerifier::new(&root, VerificationPolicy::strict(120).unwrap());
        let dependency = vec![CandidateFile::new(
            "Cargo.toml",
            "[package]\nname=\"bad\"\nversion=\"0.1.0\"\n[dependencies]\nserde=\"1\"\n",
        )
        .unwrap()];
        assert!(matches!(
            verifier.verify_attempt("work-111111111111111111111111", 1, &dependency),
            Err(VerificationError::UnsupportedCandidate(_))
        ));
        let build_script = vec![
            CandidateFile::new("src/main.rs", "fn main() {}\n").unwrap(),
            CandidateFile::new("build.rs", "fn main() {}\n").unwrap(),
        ];
        assert!(matches!(
            verifier.verify_attempt("work-222222222222222222222222", 1, &build_script),
            Err(VerificationError::UnsupportedCandidate(_))
        ));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn malformed_json_is_repaired_then_reverified_by_real_gates() {
        let root = temp_root("json-repair");
        let verifier = CandidateVerifier::new(&root, VerificationPolicy::strict(120).unwrap());
        let plan_id = "work-0123456789abcdef01234567";
        let invalid = vec![CandidateFile::new("app-spec.json", "{\"title\":}").unwrap()];
        let failed = verifier.verify_attempt(plan_id, 1, &invalid).unwrap();
        assert!(!failed.passed);
        assert!(failed.failure.contains("app-spec.json"));
        assert!(failed.failure.contains("strict parsing"));

        let valid =
            vec![
                CandidateFile::new("app-spec.json", "{\"title\":\"Task Board\",\"tasks\":[]}")
                    .unwrap(),
            ];
        let repair = verifier
            .store_repair(&failed, "json-repair-model", &valid)
            .unwrap();
        let passed = verifier.verify_attempt(plan_id, 2, &repair.files).unwrap();
        assert!(passed.passed, "{}", passed.failure);
        let success = verifier.finalize(&passed).unwrap();
        let verified = verify_candidate_evidence(
            &success.manifest_path,
            &success.manifest_hash,
            plan_id,
            &success.bundle_hash,
        )
        .unwrap();
        assert_eq!(verified.attempts, 2);
        assert_eq!(verified.repairs, 1);
        fs::remove_dir_all(root).unwrap();
    }
}
