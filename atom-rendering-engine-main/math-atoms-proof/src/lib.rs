//! Strict proof records and durable JSONL persistence.

use math_atoms_json::{parse as parse_json, JsonValue};
use math_atoms_lock::{acquire_file_lease, FileLease};
use math_atoms_secrets::redact_sensitive_text;
use math_atoms_verification::CandidateVerificationEvidence;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOCK_TIMEOUT: Duration = Duration::from_secs(4);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofRecord {
    pub recipe_id: String,
    pub status: String,
    pub atoms: Vec<String>,
    pub evidence_count: usize,
    pub blockers: Vec<String>,
    pub provider_state: String,
    pub provider_model: String,
    pub provider_endpoint: String,
    pub provider_output_artifact: String,
    pub provider_output_hash: String,
    pub provider_output_len: usize,
    pub work_plan_id: String,
    pub work_plan_manifest: String,
    pub work_packet_count: usize,
    pub candidate_verification: Option<CandidateVerificationEvidence>,
    pub route_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofStore {
    path: PathBuf,
}

struct StoreLock {
    _lease: FileLease,
}

impl ProofStore {
    pub fn default_path() -> PathBuf {
        let base = std::env::var("MATH_ATOMS_STORE_DIR")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("LOCALAPPDATA").map(PathBuf::from))
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("MathAtomsCoder").join("proofs.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, record: &ProofRecord) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = acquire_lock(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)?;
        let start = file.metadata()?.len();
        let line = format!("{}\n", record.to_json());
        file.write_all(line.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        file.seek(SeekFrom::Start(start))?;
        let mut readback = vec![0; line.len()];
        file.read_exact(&mut readback)?;
        if readback != line.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "proof append readback mismatch",
            ));
        }
        Ok(())
    }

    pub fn read_to_string(&self) -> io::Result<String> {
        if !self.path.exists() {
            return Ok(String::new());
        }
        let _lock = acquire_lock(&self.path)?;
        let mut file = match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    pub fn read_records(&self) -> io::Result<Vec<ProofRecord>> {
        let text = self.read_to_string()?;
        let mut records = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let Some(record) = ProofRecord::from_json(line) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid proof store record at line {}", idx + 1),
                ));
            };
            records.push(record);
        }
        Ok(records)
    }
}

fn acquire_lock(store_path: &Path) -> io::Result<StoreLock> {
    let path = lock_path(store_path);
    let lease = acquire_file_lease(&path, LOCK_TIMEOUT, STALE_LOCK_AGE)?;
    Ok(StoreLock { _lease: lease })
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("proofs.jsonl");
    path.with_file_name(format!("{name}.lock"))
}

impl ProofRecord {
    pub fn to_json(&self) -> String {
        let recipe_id = redact_sensitive_text(&self.recipe_id);
        let status = redact_sensitive_text(&self.status);
        let atoms = self
            .atoms
            .iter()
            .map(|value| redact_sensitive_text(value))
            .collect::<Vec<_>>();
        let blockers = self
            .blockers
            .iter()
            .map(|value| redact_sensitive_text(value))
            .collect::<Vec<_>>();
        let provider_state = redact_sensitive_text(&self.provider_state);
        let provider_model = redact_sensitive_text(&self.provider_model);
        let provider_endpoint = redact_sensitive_text(&self.provider_endpoint);
        let provider_output_artifact = redact_sensitive_text(&self.provider_output_artifact);
        let provider_output_hash = redact_sensitive_text(&self.provider_output_hash);
        let work_plan_id = redact_sensitive_text(&self.work_plan_id);
        let work_plan_manifest = redact_sensitive_text(&self.work_plan_manifest);
        let candidate_verification = self
            .candidate_verification
            .as_ref()
            .map(candidate_verification_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"recipe_id\":\"{}\",\"status\":\"{}\",\"atoms\":[{}],\"evidence_count\":{},\"blockers\":[{}],\"provider_state\":\"{}\",\"provider_model\":\"{}\",\"provider_endpoint\":\"{}\",\"provider_output_artifact\":\"{}\",\"provider_output_hash\":\"{}\",\"provider_output_len\":{},\"work_plan_id\":\"{}\",\"work_plan_manifest\":\"{}\",\"work_packet_count\":{},\"candidate_verification\":{},\"route_len\":{}}}",
            escape(&recipe_id),
            escape(&status),
            string_array(&atoms),
            self.evidence_count,
            string_array(&blockers),
            escape(&provider_state),
            escape(&provider_model),
            escape(&provider_endpoint),
            escape(&provider_output_artifact),
            escape(&provider_output_hash),
            self.provider_output_len,
            escape(&work_plan_id),
            escape(&work_plan_manifest),
            self.work_packet_count,
            candidate_verification,
            self.route_len
        )
    }

    pub fn from_json(line: &str) -> Option<Self> {
        const ALLOWED_FIELDS: [&str; 16] = [
            "recipe_id",
            "status",
            "atoms",
            "evidence_count",
            "blockers",
            "provider_state",
            "provider_model",
            "provider_endpoint",
            "provider_output_artifact",
            "provider_output_hash",
            "provider_output_len",
            "work_plan_id",
            "work_plan_manifest",
            "work_packet_count",
            "candidate_verification",
            "route_len",
        ];
        let root = parse_json(line).ok()?;
        let object = root.as_object()?;
        if object
            .iter()
            .any(|(name, _)| !ALLOWED_FIELDS.contains(&name.as_str()))
        {
            return None;
        }
        Some(Self {
            recipe_id: json_string(&root, "recipe_id")?,
            status: json_string(&root, "status")?,
            atoms: json_string_array(&root, "atoms")?,
            evidence_count: json_usize(&root, "evidence_count")?,
            blockers: json_string_array(&root, "blockers")?,
            provider_state: json_string(&root, "provider_state")?,
            provider_model: json_string(&root, "provider_model").unwrap_or_default(),
            provider_endpoint: json_string(&root, "provider_endpoint").unwrap_or_default(),
            provider_output_artifact: json_string(&root, "provider_output_artifact")
                .unwrap_or_default(),
            provider_output_hash: json_string(&root, "provider_output_hash").unwrap_or_default(),
            provider_output_len: json_usize(&root, "provider_output_len").unwrap_or(0),
            work_plan_id: json_string(&root, "work_plan_id").unwrap_or_default(),
            work_plan_manifest: json_string(&root, "work_plan_manifest").unwrap_or_default(),
            work_packet_count: json_usize(&root, "work_packet_count").unwrap_or(0),
            candidate_verification: json_candidate_verification(&root)?,
            route_len: json_usize(&root, "route_len")?,
        })
    }
}

fn string_array(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{}\"", escape(value)))
        .collect::<Vec<_>>()
        .join(",")
}

fn escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
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

fn json_string(root: &JsonValue, key: &str) -> Option<String> {
    root.get(key)?.as_str().map(str::to_string)
}

fn json_string_array(root: &JsonValue, key: &str) -> Option<Vec<String>> {
    root.get(key)?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn json_usize(root: &JsonValue, key: &str) -> Option<usize> {
    root.get(key)?.as_u64()?.try_into().ok()
}

fn candidate_verification_json(evidence: &CandidateVerificationEvidence) -> String {
    format!(
        "{{\"manifest_path\":\"{}\",\"manifest_hash\":\"{}\",\"bundle_hash\":\"{}\",\"attempts\":{},\"repairs\":{}}}",
        escape(&redact_sensitive_text(&evidence.manifest_path)),
        escape(&redact_sensitive_text(&evidence.manifest_hash)),
        escape(&redact_sensitive_text(&evidence.bundle_hash)),
        evidence.attempts,
        evidence.repairs
    )
}

fn json_candidate_verification(root: &JsonValue) -> Option<Option<CandidateVerificationEvidence>> {
    let Some(value) = root.get("candidate_verification") else {
        return Some(None);
    };
    if matches!(value, JsonValue::Null) {
        return Some(None);
    }
    let object = value.as_object()?;
    let fields = [
        "manifest_path",
        "manifest_hash",
        "bundle_hash",
        "attempts",
        "repairs",
    ];
    if object.len() != fields.len()
        || object
            .iter()
            .any(|(name, _)| !fields.contains(&name.as_str()))
    {
        return None;
    }
    Some(Some(CandidateVerificationEvidence {
        manifest_path: json_string(value, "manifest_path")?,
        manifest_hash: json_string(value, "manifest_hash")?,
        bundle_hash: json_string(value, "bundle_hash")?,
        attempts: value.get("attempts")?.as_u64()?.try_into().ok()?,
        repairs: value.get("repairs")?.as_u64()?.try_into().ok()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_store_appends_jsonl_record() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-proof-store-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let record = ProofRecord {
            recipe_id: "production-app-runtime".to_string(),
            status: "proven".to_string(),
            atoms: vec!["flow".to_string(), "measure".to_string()],
            evidence_count: 3,
            blockers: vec!["provider token = hunter2".to_string()],
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: "C:/audit/provider.txt".to_string(),
            provider_output_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            provider_output_len: 18,
            work_plan_id: "work-proof-fixture".to_string(),
            work_plan_manifest: "C:/audit/plan-expanded.json".to_string(),
            work_packet_count: 13,
            candidate_verification: Some(CandidateVerificationEvidence {
                manifest_path: "C:/audit/verification-final.json".to_string(),
                manifest_hash:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                bundle_hash:
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                attempts: 2,
                repairs: 1,
            }),
            route_len: 4,
        };
        store.append(&record).unwrap();
        let text = store.read_to_string().unwrap();
        fs::remove_file(&path).ok();
        assert!(text.contains("\"recipe_id\":\"production-app-runtime\""));
        assert!(text.contains("\"provider_state\":\"provider:ran\""));
        assert!(text.contains("\"provider_model\":\"gpt-test\""));
        assert!(text.contains("\"provider_output_artifact\":\"C:/audit/provider.txt\""));
        assert!(text.contains("\"provider_output_hash\":\"sha256:0123456789abcdef"));
        assert!(text.contains("\"provider_output_len\":18"));
        assert!(text.contains("\"candidate_verification\":{\"manifest_path\""));
        assert!(text.contains("\"attempts\":2,\"repairs\":1"));
        assert!(!text.contains("hunter2"));
        assert!(text.contains("provider token = [REDACTED]"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn proof_store_reads_jsonl_records() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-proof-store-read-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        let record = ProofRecord {
            recipe_id: "wiki-graph-rag".to_string(),
            status: "proven".to_string(),
            atoms: vec!["scan".to_string(), "hash".to_string()],
            evidence_count: 7,
            blockers: vec!["none".to_string()],
            provider_state: "provider:ran".to_string(),
            provider_model: "fake-responsive-provider".to_string(),
            provider_endpoint: "http://127.0.0.1:1/v1/responses".to_string(),
            provider_output_artifact: "C:/audit/provider.txt".to_string(),
            provider_output_hash:
                "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    .to_string(),
            provider_output_len: 16,
            work_plan_id: "work-proof-read-fixture".to_string(),
            work_plan_manifest: "C:/audit/plan-expanded.json".to_string(),
            work_packet_count: 13,
            candidate_verification: None,
            route_len: 5,
        };
        store.append(&record).unwrap();
        let records = store.read_records().unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(records, vec![record]);
    }

    #[test]
    fn proof_store_rejects_corrupt_jsonl_records() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-proof-store-corrupt-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ProofStore::new(&path);
        fs::write(&path, "{\"recipe_id\":\"ok\"}\nnot-json\n").unwrap();
        let error = store.read_records().unwrap_err();
        fs::remove_file(&path).ok();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 1"));

        let valid = ProofRecord {
            recipe_id: "wiki-graph-rag".to_string(),
            status: "blocked".to_string(),
            atoms: vec!["scan".to_string()],
            evidence_count: 1,
            blockers: vec!["test".to_string()],
            provider_state: "provider:blocked".to_string(),
            provider_model: String::new(),
            provider_endpoint: String::new(),
            provider_output_artifact: String::new(),
            provider_output_hash: String::new(),
            provider_output_len: 0,
            work_plan_id: String::new(),
            work_plan_manifest: String::new(),
            work_packet_count: 0,
            candidate_verification: None,
            route_len: 4,
        };
        fs::write(&path, format!("{} trailing\n", valid.to_json())).unwrap();
        assert_eq!(
            store.read_records().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(&path).ok();
    }

    #[test]
    fn proof_store_reads_legacy_records_without_provider_audit_fields() {
        let line = "{\"recipe_id\":\"wiki-graph-rag\",\"status\":\"proven\",\"atoms\":[\"scan\"],\"evidence_count\":7,\"blockers\":[],\"provider_state\":\"provider:ran\",\"route_len\":5}";
        let record = ProofRecord::from_json(line).unwrap();
        assert_eq!(record.provider_model, "");
        assert_eq!(record.provider_endpoint, "");
        assert_eq!(record.provider_output_artifact, "");
        assert_eq!(record.provider_output_hash, "");
        assert_eq!(record.provider_output_len, 0);
        assert_eq!(record.work_plan_id, "");
        assert_eq!(record.work_plan_manifest, "");
        assert_eq!(record.work_packet_count, 0);
        assert_eq!(record.candidate_verification, None);
        assert_eq!(record.route_len, 5);
    }

    #[test]
    fn concurrent_proof_writers_remain_complete_and_parseable() {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-proof-concurrent-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = std::sync::Arc::new(ProofStore::new(&path));
        let mut workers = Vec::new();
        for worker in 0..8 {
            let store = store.clone();
            workers.push(std::thread::spawn(move || {
                for index in 0..8 {
                    store
                        .append(&ProofRecord {
                            recipe_id: format!("wiki-graph-rag-{worker}-{index}"),
                            status: "blocked".to_string(),
                            atoms: vec!["scan".to_string()],
                            evidence_count: 1,
                            blockers: vec!["fixture".to_string()],
                            provider_state: "provider:blocked".to_string(),
                            provider_model: String::new(),
                            provider_endpoint: String::new(),
                            provider_output_artifact: String::new(),
                            provider_output_hash: String::new(),
                            provider_output_len: 0,
                            work_plan_id: String::new(),
                            work_plan_manifest: String::new(),
                            work_packet_count: 0,
                            candidate_verification: None,
                            route_len: 4,
                        })
                        .unwrap();
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let records = store.read_records().unwrap();
        fs::remove_file(path).ok();
        assert_eq!(records.len(), 64);
        assert_eq!(
            records
                .iter()
                .map(|record| record.recipe_id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            64
        );
    }
}
