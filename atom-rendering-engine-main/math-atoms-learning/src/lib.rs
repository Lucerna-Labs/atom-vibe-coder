//! Durable learning events for Atom Vibe Coder.
//!
//! The ledger keeps failed gate evidence separate from proof while retaining enough
//! structured context to correct future attempts. Successful, gate-passing artifacts
//! can be promoted as reusable graph evidence by the runtime.

use math_atoms_hash::{sha256_file, valid_sha256_tag};
use math_atoms_json::{parse as parse_json, JsonValue};
use math_atoms_secrets::redact_sensitive_text;
use math_atoms_work::verify_work_plan_evidence;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LEARNING_SCHEMA_VERSION: u32 = 3;
const LEGACY_LEARNING_SCHEMA_VERSION: u32 = 1;
const LEGACY_SHA_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_GRAPH_MEMORY_LIMIT: usize = 256;
const MAX_SHORT_FIELD: usize = 512;
const MAX_LONG_FIELD: usize = 4_096;
const LOCK_RETRIES: usize = 200;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LearningOutcome {
    Failed,
    Succeeded,
}

impl LearningOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Succeeded => "succeeded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "failed" | "failure" | "blocked" => Some(Self::Failed),
            "succeeded" | "success" | "proven" | "passed" => Some(Self::Succeeded),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearningRecord {
    pub schema_version: u32,
    pub id: String,
    pub timestamp_ms: u64,
    pub source: String,
    pub intent: String,
    pub recipe_id: String,
    pub atom_stack: Vec<String>,
    pub gate: String,
    pub attempt: u32,
    pub outcome: LearningOutcome,
    pub failure: String,
    pub correction: String,
    pub artifact_path: String,
    pub artifact_hash: String,
    pub provider_model: String,
    pub work_plan_id: String,
    pub work_plan_manifest: String,
    pub work_packet_count: usize,
    pub route_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearningRecordInput {
    pub source: String,
    pub intent: String,
    pub recipe_id: String,
    pub atom_stack: Vec<String>,
    pub gate: String,
    pub attempt: u32,
    pub outcome: LearningOutcome,
    pub failure: String,
    pub correction: String,
    pub artifact_path: String,
    pub artifact_hash: String,
    pub provider_model: String,
    pub work_plan_id: String,
    pub work_plan_manifest: String,
    pub work_packet_count: usize,
    pub route_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearningHit {
    pub record_id: String,
    pub title: String,
    pub excerpt: String,
    pub score: i32,
    pub outcome: LearningOutcome,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LearningSummary {
    pub total: usize,
    pub failed: usize,
    pub succeeded: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearningStore {
    path: PathBuf,
}

struct StoreLock {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

impl LearningRecord {
    pub fn new(input: LearningRecordInput) -> Self {
        let timestamp_ms = now_ms();
        let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let id_seed = format!(
            "{}:{}:{}:{}:{}:{}",
            timestamp_ms,
            std::process::id(),
            sequence,
            input.source,
            input.gate,
            input.attempt
        );
        Self {
            schema_version: LEARNING_SCHEMA_VERSION,
            id: format!("{:016x}", fnv1a(id_seed.as_bytes())),
            timestamp_ms,
            source: sanitize_text(&input.source, MAX_SHORT_FIELD),
            intent: sanitize_text(&input.intent, MAX_LONG_FIELD),
            recipe_id: sanitize_text(&input.recipe_id, MAX_SHORT_FIELD),
            atom_stack: input
                .atom_stack
                .iter()
                .map(|value| sanitize_text(value, MAX_SHORT_FIELD))
                .collect(),
            gate: sanitize_text(&input.gate, MAX_SHORT_FIELD),
            attempt: input.attempt,
            outcome: input.outcome,
            failure: sanitize_text(&input.failure, MAX_LONG_FIELD),
            correction: sanitize_text(&input.correction, MAX_LONG_FIELD),
            artifact_path: sanitize_text(&input.artifact_path, MAX_LONG_FIELD),
            artifact_hash: sanitize_text(&input.artifact_hash, MAX_SHORT_FIELD),
            provider_model: sanitize_text(&input.provider_model, MAX_SHORT_FIELD),
            work_plan_id: sanitize_text(&input.work_plan_id, MAX_SHORT_FIELD),
            work_plan_manifest: sanitize_text(&input.work_plan_manifest, MAX_LONG_FIELD),
            work_packet_count: input.work_packet_count,
            route_len: input.route_len,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_structure()?;
        if self.outcome == LearningOutcome::Succeeded
            && self.schema_version == LEARNING_SCHEMA_VERSION
            && self.requires_work_evidence()
        {
            let verified = verify_work_plan_evidence(
                &self.work_plan_manifest,
                &self.work_plan_id,
                self.work_packet_count,
            )
            .map_err(|error| format!("work plan evidence failed: {error}"))?;
            if !self.provider_model.trim().is_empty() && verified.model != self.provider_model {
                return Err(
                    "learning provider model does not match work packet evidence".to_string(),
                );
            }
        }
        Ok(())
    }

    pub fn validate_structure(&self) -> Result<(), String> {
        if !matches!(
            self.schema_version,
            LEGACY_LEARNING_SCHEMA_VERSION | LEGACY_SHA_SCHEMA_VERSION | LEARNING_SCHEMA_VERSION
        ) {
            return Err(format!(
                "unsupported learning schema version {}",
                self.schema_version
            ));
        }
        for (name, value) in [
            ("id", self.id.as_str()),
            ("source", self.source.as_str()),
            ("intent", self.intent.as_str()),
            ("gate", self.gate.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("learning {name} must not be empty"));
            }
        }
        if self.timestamp_ms == 0 || self.attempt == 0 {
            return Err("learning timestamp and attempt must be positive".to_string());
        }
        if self.atom_stack.iter().any(|atom| atom.trim().is_empty()) {
            return Err("learning atom stack contains an empty atom".to_string());
        }
        if self.outcome == LearningOutcome::Failed && self.failure.trim().is_empty() {
            return Err("failed learning record requires failure evidence".to_string());
        }
        let native_route_success = self.source == "native-app" && self.route_len >= 4;
        if self.outcome == LearningOutcome::Succeeded {
            if self.schema_version == LEGACY_LEARNING_SCHEMA_VERSION {
                if !native_route_success && self.artifact_hash.trim().is_empty() {
                    return Err(
                        "legacy successful learning requires a full route or checksum".to_string(),
                    );
                }
            } else {
                let native_non_provider_success =
                    native_route_success && self.provider_model.trim().is_empty();
                if !native_non_provider_success
                    && (self.artifact_path.trim().is_empty()
                        || self.artifact_hash.trim().is_empty())
                {
                    return Err(
                        "successful provider or harness learning requires an artifact path and hash"
                            .to_string(),
                    );
                }
                if self.schema_version == LEARNING_SCHEMA_VERSION
                    && self.requires_work_evidence()
                    && (self.work_plan_id.trim().is_empty()
                        || self.work_plan_manifest.trim().is_empty()
                        || self.work_packet_count < 13)
                {
                    return Err(
                        "successful provider learning requires a verified meticulous work plan"
                            .to_string(),
                    );
                }
            }
        }
        if !self.artifact_hash.is_empty() {
            let hash_valid = if self.schema_version == LEGACY_LEARNING_SCHEMA_VERSION {
                valid_legacy_fnv_hash(&self.artifact_hash)
            } else {
                valid_sha256_tag(&self.artifact_hash)
            };
            if !hash_valid {
                return Err("learning artifact hash does not match its schema".to_string());
            }
        }
        Ok(())
    }

    pub fn is_promotable_success(&self) -> bool {
        if self.validate().is_err() || self.outcome != LearningOutcome::Succeeded {
            return false;
        }
        if self.source == "native-app"
            && self.provider_model.trim().is_empty()
            && self.route_len >= 4
        {
            return true;
        }
        self.schema_version == LEARNING_SCHEMA_VERSION
            && valid_sha256_tag(&self.artifact_hash)
            && sha256_file(&self.artifact_path)
                .map(|actual| actual == self.artifact_hash)
                .unwrap_or(false)
            && (!self.requires_work_evidence()
                || verify_work_plan_evidence(
                    &self.work_plan_manifest,
                    &self.work_plan_id,
                    self.work_packet_count,
                )
                .map(|plan| {
                    self.provider_model.trim().is_empty() || plan.model == self.provider_model
                })
                .unwrap_or(false))
    }

    pub fn node_id(&self) -> String {
        format!("learning:{}:{}", self.outcome.as_str(), self.id)
    }

    pub fn title(&self) -> String {
        match self.outcome {
            LearningOutcome::Failed => format!("Learned failure: {}", self.gate),
            LearningOutcome::Succeeded => format!("Reusable success: {}", self.gate),
        }
    }

    pub fn excerpt(&self) -> String {
        match self.outcome {
            LearningOutcome::Failed => format!(
                "Attempt {} for '{}' failed gate {}: {}. Correct this failure and rerun the real gate before claiming success.",
                self.attempt,
                brief(&self.intent, 240),
                self.gate,
                brief(&self.failure, 360)
            ),
            LearningOutcome::Succeeded => {
                let correction = if self.correction.trim().is_empty() {
                    "No prior failure was required.".to_string()
                } else {
                    format!("Correction applied: {}.", self.correction)
                };
                format!(
                    "Attempt {} for '{}' passed gate {} with recipe {} and artifact hash {}. {}",
                    self.attempt,
                    brief(&self.intent, 240),
                    self.gate,
                    empty_as(&self.recipe_id, "unspecified"),
                    empty_as(&self.artifact_hash, "route-audited"),
                    brief(&correction, 360)
                )
            }
        }
    }

    pub fn tags(&self) -> Vec<String> {
        let mut values = vec![
            "learning".to_string(),
            self.outcome.as_str().to_string(),
            self.source.to_ascii_lowercase(),
            self.gate.to_ascii_lowercase(),
            self.recipe_id.to_ascii_lowercase(),
            self.provider_model.to_ascii_lowercase(),
        ];
        values.extend(
            self.atom_stack
                .iter()
                .map(|value| value.to_ascii_lowercase()),
        );
        values.extend(tokenize(&self.intent).into_iter().take(24));
        values.retain(|value| !value.trim().is_empty());
        values.sort();
        values.dedup();
        values
    }

    pub fn memory_key(&self) -> String {
        let failure = tokenize(&self.failure)
            .into_iter()
            .take(24)
            .collect::<Vec<_>>()
            .join("-");
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.source,
            self.gate,
            self.recipe_id,
            self.outcome.as_str(),
            tokenize(&self.intent).join("-"),
            failure
        )
    }

    fn requires_work_evidence(&self) -> bool {
        !self.provider_model.trim().is_empty()
            || self.source.starts_with("provider-")
            || self.source.starts_with("deepseek-")
    }

    pub fn to_json(&self) -> String {
        let prefix = format!(
            "{{\"schema_version\":{},\"id\":\"{}\",\"timestamp_ms\":{},\"source\":\"{}\",\"intent\":\"{}\",\"recipe_id\":\"{}\",\"atom_stack\":[{}],\"gate\":\"{}\",\"attempt\":{},\"outcome\":\"{}\",\"failure\":\"{}\",\"correction\":\"{}\",\"artifact_path\":\"{}\",\"artifact_hash\":\"{}\",\"provider_model\":\"{}\"",
            self.schema_version,
            escape(&self.id),
            self.timestamp_ms,
            escape(&self.source),
            escape(&self.intent),
            escape(&self.recipe_id),
            string_array(&self.atom_stack),
            escape(&self.gate),
            self.attempt,
            self.outcome.as_str(),
            escape(&self.failure),
            escape(&self.correction),
            escape(&self.artifact_path),
            escape(&self.artifact_hash),
            escape(&self.provider_model)
        );
        if self.schema_version < LEARNING_SCHEMA_VERSION {
            return format!("{prefix},\"route_len\":{}}}", self.route_len);
        }
        format!(
            "{prefix},\"work_plan_id\":\"{}\",\"work_plan_manifest\":\"{}\",\"work_packet_count\":{},\"route_len\":{}}}",
            escape(&self.work_plan_id),
            escape(&self.work_plan_manifest),
            self.work_packet_count,
            self.route_len
        )
    }

    pub fn from_json(line: &str) -> Option<Self> {
        const BASE_FIELDS: [&str; 16] = [
            "schema_version",
            "id",
            "timestamp_ms",
            "source",
            "intent",
            "recipe_id",
            "atom_stack",
            "gate",
            "attempt",
            "outcome",
            "failure",
            "correction",
            "artifact_path",
            "artifact_hash",
            "provider_model",
            "route_len",
        ];
        const WORK_FIELDS: [&str; 3] = ["work_plan_id", "work_plan_manifest", "work_packet_count"];
        let root = parse_json(line).ok()?;
        let object = root.as_object()?;
        let schema_version = json_u32(&root, "schema_version")?;
        let uses_work_schema = schema_version == LEARNING_SCHEMA_VERSION;
        let expected_len = BASE_FIELDS.len() + usize::from(uses_work_schema) * WORK_FIELDS.len();
        if object.len() != expected_len
            || object.iter().any(|(name, _)| {
                !BASE_FIELDS.contains(&name.as_str())
                    && (!uses_work_schema || !WORK_FIELDS.contains(&name.as_str()))
            })
        {
            return None;
        }
        let record = Self {
            schema_version,
            id: json_string(&root, "id")?,
            timestamp_ms: json_u64(&root, "timestamp_ms")?,
            source: json_string(&root, "source")?,
            intent: json_string(&root, "intent")?,
            recipe_id: json_string(&root, "recipe_id")?,
            atom_stack: json_string_array(&root, "atom_stack")?,
            gate: json_string(&root, "gate")?,
            attempt: json_u32(&root, "attempt")?,
            outcome: LearningOutcome::parse(&json_string(&root, "outcome")?)?,
            failure: json_string(&root, "failure")?,
            correction: json_string(&root, "correction")?,
            artifact_path: json_string(&root, "artifact_path")?,
            artifact_hash: json_string(&root, "artifact_hash")?,
            provider_model: json_string(&root, "provider_model")?,
            work_plan_id: json_string(&root, "work_plan_id").unwrap_or_default(),
            work_plan_manifest: json_string(&root, "work_plan_manifest").unwrap_or_default(),
            work_packet_count: json_usize(&root, "work_packet_count").unwrap_or(0),
            route_len: json_usize(&root, "route_len")?,
        };
        record.validate_structure().ok()?;
        Some(record)
    }
}

impl LearningSummary {
    pub fn from_records(records: &[LearningRecord]) -> Self {
        let mut summary = Self {
            total: records.len(),
            ..Self::default()
        };
        for record in records {
            match record.outcome {
                LearningOutcome::Failed => summary.failed += 1,
                LearningOutcome::Succeeded => summary.succeeded += 1,
            }
        }
        summary
    }
}

impl LearningStore {
    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var("MATH_ATOMS_LEARNING_STORE") {
            if !path.trim().is_empty() {
                return PathBuf::from(path);
            }
        }
        let base = std::env::var("MATH_ATOMS_STORE_DIR")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("LOCALAPPDATA").map(PathBuf::from))
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("MathAtomsCoder").join("learning.jsonl")
    }

    pub fn beside(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("proofs.jsonl");
        let learning_name = if file_name.eq_ignore_ascii_case("proofs.jsonl") {
            "learning.jsonl".to_string()
        } else {
            format!("{file_name}.learning.jsonl")
        };
        let learning_path = path.with_file_name(learning_name);
        Self::new(learning_path)
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, record: &LearningRecord) -> io::Result<()> {
        record
            .validate()
            .map_err(|reason| io::Error::new(io::ErrorKind::InvalidInput, reason))?;
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let _lock = self.acquire_lock()?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)?;
        let start = file.metadata()?.len();
        let encoded = format!("{}\n", record.to_json());
        file.write_all(encoded.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        file.seek(SeekFrom::Start(start))?;
        let mut readback = vec![0; encoded.len()];
        file.read_exact(&mut readback)?;
        if readback != encoded.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "learning append verification did not match persisted bytes",
            ));
        }
        Ok(())
    }

    pub fn read_to_string(&self) -> io::Result<String> {
        let _lock = self.acquire_lock()?;
        if !self.path.exists() {
            return Ok(String::new());
        }
        let mut file = match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    pub fn read_records(&self) -> io::Result<Vec<LearningRecord>> {
        let text = self.read_to_string()?;
        let mut records = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let Some(record) = LearningRecord::from_json(line) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid learning store record at line {}", index + 1),
                ));
            };
            records.push(record);
        }
        Ok(records)
    }

    fn acquire_lock(&self) -> io::Result<StoreLock> {
        let lock_path = lock_path(&self.path);
        if let Some(parent) = lock_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        for _ in 0..LOCK_RETRIES {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(file) => {
                    return Ok(StoreLock {
                        path: lock_path,
                        file: Some(file),
                    })
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::AlreadyExists
                            | io::ErrorKind::PermissionDenied
                            | io::ErrorKind::WouldBlock
                    ) =>
                {
                    if let Err(lock_error) = reclaim_stale_lock(&lock_path) {
                        if lock_error.kind() != io::ErrorKind::PermissionDenied {
                            return Err(lock_error);
                        }
                    }
                    thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("learning store lock timed out at {}", lock_path.display()),
        ))
    }
}

pub fn effective_records(records: &[LearningRecord], limit: usize) -> Vec<LearningRecord> {
    if limit == 0 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for record in records.iter().rev() {
        if seen.insert(record.memory_key()) {
            selected.push(record.clone());
            if selected.len() == limit {
                break;
            }
        }
    }
    selected.reverse();
    selected
}

pub fn rank_records(
    records: &[LearningRecord],
    query: &str,
    atom_stack: &[String],
    limit: usize,
) -> Vec<LearningHit> {
    let query_tokens: HashSet<String> = tokenize(query).into_iter().collect();
    let atoms: HashSet<String> = atom_stack
        .iter()
        .map(|atom| atom.to_ascii_lowercase())
        .collect();
    let mut hits: Vec<(u64, LearningHit)> = records
        .iter()
        .filter_map(|record| {
            let record_tokens: HashSet<String> = tokenize(&record.intent).into_iter().collect();
            let token_overlap = query_tokens.intersection(&record_tokens).count() as i32;
            let atom_overlap = record
                .atom_stack
                .iter()
                .filter(|atom| atoms.contains(&atom.to_ascii_lowercase()))
                .count() as i32;
            let gate_overlap = tokenize(&record.gate)
                .iter()
                .filter(|token| query_tokens.contains(*token))
                .count() as i32;
            let recipe_overlap = tokenize(&record.recipe_id)
                .iter()
                .filter(|token| query_tokens.contains(*token))
                .count() as i32;
            let score = token_overlap * 6
                + atom_overlap * 8
                + gate_overlap * 5
                + recipe_overlap * 4
                + i32::from(record.outcome == LearningOutcome::Succeeded) * 2;
            (score > 0).then(|| {
                (
                    record.timestamp_ms,
                    LearningHit {
                        record_id: record.id.clone(),
                        title: record.title(),
                        excerpt: record.excerpt(),
                        score,
                        outcome: record.outcome,
                    },
                )
            })
        })
        .collect();
    hits.sort_by(|(a_time, a), (b_time, b)| {
        b.score
            .cmp(&a.score)
            .then_with(|| b_time.cmp(a_time))
            .then_with(|| a.record_id.cmp(&b.record_id))
    });
    hits.truncate(limit);
    hits.into_iter().map(|(_, hit)| hit).collect()
}

pub fn artifact_hash(path: impl AsRef<Path>) -> io::Result<String> {
    sha256_file(path)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn valid_legacy_fnv_hash(value: &str) -> bool {
    value.len() == "fnv:0000000000000000".len()
        && value.starts_with("fnv:")
        && value.chars().skip(4).all(|ch| ch.is_ascii_hexdigit())
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|part| part.len() > 1)
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn sanitize_text(value: &str, limit: usize) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
                ' '
            } else {
                ch
            }
        })
        .take(limit)
        .collect();
    redact_sensitive_text(&cleaned)
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("learning.jsonl");
    path.with_file_name(format!("{name}.lock"))
}

fn reclaim_stale_lock(path: &Path) -> io::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let stale = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > STALE_LOCK_AGE);
    if stale {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn empty_as<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn brief(value: &str, limit: usize) -> String {
    let mut text: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        text.push_str("...");
    }
    text
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

fn json_u64(root: &JsonValue, key: &str) -> Option<u64> {
    root.get(key)?.as_u64()
}

fn json_u32(root: &JsonValue, key: &str) -> Option<u32> {
    json_u64(root, key)?.try_into().ok()
}

fn json_usize(root: &JsonValue, key: &str) -> Option<usize> {
    json_u64(root, key)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "math-atoms-learning-{label}-{}-{}.jsonl",
            std::process::id(),
            now_ms()
        ))
    }

    fn record(outcome: LearningOutcome, attempt: u32) -> LearningRecord {
        LearningRecord::new(LearningRecordInput {
            source: "test-harness".to_string(),
            intent: "Build a Bluetooth driver".to_string(),
            recipe_id: "provider-model-loop".to_string(),
            atom_stack: vec!["scan".to_string(), "measure".to_string()],
            gate: "bluetooth-driver".to_string(),
            attempt,
            outcome,
            failure: if outcome == LearningOutcome::Failed {
                "missing rejected-address behavior".to_string()
            } else {
                String::new()
            },
            correction: if outcome == LearningOutcome::Succeeded {
                "added rejected-address behavior".to_string()
            } else {
                String::new()
            },
            artifact_path: "driver.rs".to_string(),
            artifact_hash: if outcome == LearningOutcome::Succeeded {
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string()
            } else {
                String::new()
            },
            provider_model: String::new(),
            work_plan_id: String::new(),
            work_plan_manifest: String::new(),
            work_packet_count: 0,
            route_len: 4,
        })
    }

    #[test]
    fn ledger_round_trips_failed_and_successful_records() {
        let path = temp_path("roundtrip");
        let store = LearningStore::new(&path);
        let failed = record(LearningOutcome::Failed, 1);
        let succeeded = record(LearningOutcome::Succeeded, 2);
        store.append(&failed).unwrap();
        store.append(&succeeded).unwrap();
        let loaded = store.read_records().unwrap();
        fs::remove_file(&path).ok();
        assert_eq!(loaded, vec![failed, succeeded]);
        assert_eq!(
            LearningSummary::from_records(&loaded),
            LearningSummary {
                total: 2,
                failed: 1,
                succeeded: 1
            }
        );
    }

    #[test]
    fn corrupt_record_fails_closed() {
        let path = temp_path("corrupt");
        fs::write(&path, "not-json\n").unwrap();
        let error = LearningStore::new(&path).read_records().unwrap_err();
        fs::remove_file(&path).ok();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 1"));

        let mut corrupt = record(LearningOutcome::Failed, 1).to_json();
        corrupt.push_str(" trailing-garbage\n");
        fs::write(&path, corrupt).unwrap();
        assert_eq!(
            LearningStore::new(&path).read_records().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(path).ok();
    }

    #[test]
    fn effective_memory_keeps_latest_unique_lessons() {
        let first = record(LearningOutcome::Failed, 1);
        let mut duplicate = first.clone();
        duplicate.id = "duplicate".to_string();
        duplicate.timestamp_ms += 1;
        let success = record(LearningOutcome::Succeeded, 2);
        let effective = effective_records(&[first, duplicate.clone(), success.clone()], 8);
        assert_eq!(effective, vec![duplicate, success]);
    }

    #[test]
    fn relevant_failure_is_ranked_for_next_attempt() {
        let failed = record(LearningOutcome::Failed, 1);
        let unrelated = LearningRecord::new(LearningRecordInput {
            intent: "Build a weather dashboard".to_string(),
            gate: "dashboard".to_string(),
            ..LearningRecordInput {
                source: "test-harness".to_string(),
                intent: String::new(),
                recipe_id: "native-atom-renderer".to_string(),
                atom_stack: vec!["project".to_string()],
                gate: String::new(),
                attempt: 1,
                outcome: LearningOutcome::Failed,
                failure: "missing chart".to_string(),
                correction: String::new(),
                artifact_path: String::new(),
                artifact_hash: String::new(),
                provider_model: String::new(),
                work_plan_id: String::new(),
                work_plan_manifest: String::new(),
                work_packet_count: 0,
                route_len: 4,
            }
        });
        let hits = rank_records(
            &[unrelated, failed.clone()],
            "Build another Bluetooth driver",
            &["scan".to_string()],
            4,
        );
        assert_eq!(hits[0].record_id, failed.id);
        assert!(hits[0].excerpt.contains("rejected-address"));
        assert!(hits[0].excerpt.len() < 1_000);
    }

    #[test]
    fn token_like_secrets_are_redacted_before_persistence() {
        let mut input = record(LearningOutcome::Failed, 1);
        input.failure = "provider rejected sk-12345678901234567890 Bearer abcdefghijklmnop ghp_abcdefghijklmnopqrstuvwxyz token=tokenvalue123456 password: passwordvalue123456 \"api_key\":\"jsonvalue123456\" x-api-key: headervalue123456".to_string();
        let sanitized = LearningRecord::new(LearningRecordInput {
            source: input.source,
            intent: input.intent,
            recipe_id: input.recipe_id,
            atom_stack: input.atom_stack,
            gate: input.gate,
            attempt: input.attempt,
            outcome: input.outcome,
            failure: input.failure,
            correction: input.correction,
            artifact_path: input.artifact_path,
            artifact_hash: String::new(),
            provider_model: input.provider_model,
            work_plan_id: input.work_plan_id,
            work_plan_manifest: input.work_plan_manifest,
            work_packet_count: input.work_packet_count,
            route_len: input.route_len,
        });
        assert!(!sanitized.failure.contains("sk-123"));
        assert!(!sanitized.failure.contains("abcdefghijklmnop"));
        for secret in [
            "ghp_abc",
            "tokenvalue",
            "passwordvalue",
            "jsonvalue",
            "headervalue",
        ] {
            assert!(!sanitized.failure.contains(secret), "{secret}");
        }
        assert_eq!(sanitized.failure.matches("[REDACTED]").count(), 7);
    }

    #[test]
    fn every_persisted_text_surface_redacts_credentials() {
        let secret = "ghp_abcdefghijklmnopqrstuvwxyz";
        let record = LearningRecord::new(LearningRecordInput {
            source: format!("source token={secret}"),
            intent: format!("intent password={secret}"),
            recipe_id: format!("recipe api_key={secret}"),
            atom_stack: vec![format!("atom Bearer {secret}")],
            gate: format!("gate x-api-key:{secret}"),
            attempt: 1,
            outcome: LearningOutcome::Failed,
            failure: format!("failure secret={secret}"),
            correction: format!("correction token={secret}"),
            artifact_path: format!("C:/private/{secret}/artifact"),
            artifact_hash: String::new(),
            provider_model: format!("model auth={secret}"),
            work_plan_id: format!("work-plan token={secret}"),
            work_plan_manifest: format!("C:/private/{secret}/plan-expanded.json"),
            work_packet_count: 13,
            route_len: 4,
        });
        let serialized = record.to_json();
        assert!(!serialized.contains(secret));
        assert!(serialized.matches("[REDACTED]").count() >= 9);
    }

    #[test]
    fn artifact_hash_is_stable() {
        let path = temp_path("hash");
        fs::write(&path, b"artifact").unwrap();
        let first = artifact_hash(&path).unwrap();
        let second = artifact_hash(&path).unwrap();
        fs::remove_file(path).ok();
        assert_eq!(first, second);
        assert!(valid_sha256_tag(&first));
    }

    #[test]
    fn successful_harness_record_requires_hash_evidence() {
        let mut item = record(LearningOutcome::Succeeded, 1);
        item.artifact_hash.clear();
        assert!(item
            .validate()
            .unwrap_err()
            .contains("requires an artifact"));
        item.source = "native-app".to_string();
        item.provider_model.clear();
        item.artifact_path.clear();
        assert_eq!(item.validate(), Ok(()));
    }

    #[test]
    fn successful_artifact_must_recompute_before_promotion() {
        let path = temp_path("promotion-artifact");
        fs::write(&path, b"verified artifact").unwrap();
        let mut item = record(LearningOutcome::Succeeded, 1);
        item.artifact_path = path.to_string_lossy().to_string();
        item.artifact_hash = artifact_hash(&path).unwrap();
        assert!(item.is_promotable_success());
        fs::write(&path, b"tampered artifact").unwrap();
        assert!(!item.is_promotable_success());
        fs::remove_file(path).ok();
    }

    #[test]
    fn provider_success_requires_recomputable_work_packet_evidence() {
        let artifact = temp_path("provider-work-artifact");
        fs::write(&artifact, b"verified provider artifact").unwrap();
        let work_root = std::env::temp_dir().join(format!(
            "math-atoms-learning-work-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let store = math_atoms_work::WorkPlanStore::new(&work_root);
        let mut plan = math_atoms_work::WorkPlan::meticulous(
            "Build provider app",
            "provider-model-loop",
            &["measure".to_string(), "flow".to_string()],
            "learning-provider-work",
        )
        .unwrap();
        plan.expand_files(vec![math_atoms_work::WorkFile {
            path: "main.rs".to_string(),
            purpose: "provider app".to_string(),
            acceptance: vec!["runs".to_string()],
        }])
        .unwrap();
        let lease = store.acquire(&plan.id).unwrap();
        let manifest = store.write_plan_manifest(&plan).unwrap();
        for packet in &plan.packets {
            let output = match packet.contract {
                math_atoms_work::PacketContract::Envelope => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"complete\",\"checks\":[\"verified\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_work::PacketContract::FileManifest => format!(
                    "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"main.rs\",\"purpose\":\"provider app\",\"acceptance\":[\"runs\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
                    packet.id
                ),
                math_atoms_work::PacketContract::FileArtifact => {
                    "```rust\nfn main() {}\n```".to_string()
                }
            };
            store
                .store_packet(&plan, packet, &output, "deepseek-v4-pro")
                .unwrap();
        }
        drop(lease);

        let mut item = record(LearningOutcome::Succeeded, 1);
        item.source = "provider-test".to_string();
        item.provider_model = "deepseek-v4-pro".to_string();
        item.artifact_path = artifact.to_string_lossy().to_string();
        item.artifact_hash = artifact_hash(&artifact).unwrap();
        assert!(item.validate().is_err());
        item.work_plan_id = plan.id;
        item.work_plan_manifest = manifest.to_string_lossy().to_string();
        item.work_packet_count = plan.packets.len();
        assert_eq!(item.validate(), Ok(()));
        assert!(item.is_promotable_success());

        let packet_output = fs::read_dir(work_root.join(&item.work_plan_id).join("outputs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(packet_output, b"tampered").unwrap();
        assert!(!item.is_promotable_success());
        let serialized = item.to_json();
        fs::remove_dir_all(&work_root).unwrap();
        let historical = LearningRecord::from_json(&serialized).unwrap();
        assert_eq!(historical, item);
        assert!(historical.validate().is_err());
        assert!(!historical.is_promotable_success());
        fs::remove_file(artifact).ok();
    }

    #[test]
    fn legacy_checksum_records_remain_readable_but_not_promotable() {
        let mut item = record(LearningOutcome::Succeeded, 1);
        item.schema_version = LEGACY_LEARNING_SCHEMA_VERSION;
        item.artifact_path.clear();
        item.artifact_hash = "fnv:1234567890abcdef".to_string();
        let decoded = LearningRecord::from_json(&item.to_json()).unwrap();
        assert_eq!(decoded, item);
        assert!(!decoded.is_promotable_success());
    }

    #[test]
    fn custom_proof_stores_get_isolated_learning_siblings() {
        let production = LearningStore::beside("C:/state/proofs.jsonl");
        let isolated = LearningStore::beside("C:/temp/proof-test-42.jsonl");
        assert_eq!(production.path(), Path::new("C:/state/learning.jsonl"));
        assert_eq!(
            isolated.path(),
            Path::new("C:/temp/proof-test-42.jsonl.learning.jsonl")
        );
    }

    #[test]
    fn concurrent_writers_remain_complete_and_parseable() {
        let path = temp_path("concurrent");
        let store = std::sync::Arc::new(LearningStore::new(&path));
        let mut workers = Vec::new();
        for worker in 0..8 {
            let store = store.clone();
            workers.push(std::thread::spawn(move || {
                for index in 0..8 {
                    let mut item = record(LearningOutcome::Failed, worker * 8 + index + 1);
                    item.id = format!("worker-{worker}-record-{index}");
                    store.append(&item).unwrap();
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
                .map(|record| &record.id)
                .collect::<HashSet<_>>()
                .len(),
            64
        );
    }
}
