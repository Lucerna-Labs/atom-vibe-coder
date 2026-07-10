//! Durable learning events for Atom Vibe Coder.
//!
//! The ledger keeps failed gate evidence separate from proof while retaining enough
//! structured context to correct future attempts. Successful, gate-passing artifacts
//! can be promoted as reusable graph evidence by the runtime.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LEARNING_SCHEMA_VERSION: u32 = 1;
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
    _file: File,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
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
            route_len: input.route_len,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != LEARNING_SCHEMA_VERSION {
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
        if self.outcome == LearningOutcome::Succeeded
            && self.route_len < 4
            && self.artifact_hash.is_empty()
        {
            return Err(
                "successful learning record requires a full route or artifact hash".to_string(),
            );
        }
        if !self.artifact_hash.is_empty() && !valid_fnv_hash(&self.artifact_hash) {
            return Err("learning artifact hash must be fnv plus 16 hex digits".to_string());
        }
        Ok(())
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

    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":{},\"id\":\"{}\",\"timestamp_ms\":{},\"source\":\"{}\",\"intent\":\"{}\",\"recipe_id\":\"{}\",\"atom_stack\":[{}],\"gate\":\"{}\",\"attempt\":{},\"outcome\":\"{}\",\"failure\":\"{}\",\"correction\":\"{}\",\"artifact_path\":\"{}\",\"artifact_hash\":\"{}\",\"provider_model\":\"{}\",\"route_len\":{}}}",
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
            escape(&self.provider_model),
            self.route_len
        )
    }

    pub fn from_json(line: &str) -> Option<Self> {
        let record = Self {
            schema_version: u32_field(line, "schema_version")?,
            id: string_field(line, "id")?,
            timestamp_ms: u64_field(line, "timestamp_ms")?,
            source: string_field(line, "source")?,
            intent: string_field(line, "intent")?,
            recipe_id: string_field(line, "recipe_id")?,
            atom_stack: string_array_field(line, "atom_stack")?,
            gate: string_field(line, "gate")?,
            attempt: u32_field(line, "attempt")?,
            outcome: LearningOutcome::parse(&string_field(line, "outcome")?)?,
            failure: string_field(line, "failure")?,
            correction: string_field(line, "correction")?,
            artifact_path: string_field(line, "artifact_path")?,
            artifact_hash: string_field(line, "artifact_hash")?,
            provider_model: string_field(line, "provider_model")?,
            route_len: usize_field(line, "route_len")?,
        };
        record.validate().ok()?;
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
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", record.to_json())?;
        file.flush()?;
        file.sync_data()
    }

    pub fn read_to_string(&self) -> io::Result<String> {
        if !self.path.exists() {
            return Ok(String::new());
        }
        let _lock = self.acquire_lock()?;
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
        for _ in 0..LOCK_RETRIES {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(file) => {
                    return Ok(StoreLock {
                        path: lock_path,
                        _file: file,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    reclaim_stale_lock(&lock_path)?;
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
    let bytes = fs::read(path)?;
    Ok(format!("fnv:{:016x}", fnv1a(&bytes)))
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

fn valid_fnv_hash(value: &str) -> bool {
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
    redact_token_like_secrets(&cleaned)
}

fn redact_token_like_secrets(value: &str) -> String {
    let mut output = Vec::new();
    let mut redact_next = false;
    for part in value.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        let secret = redact_next
            || ((lower.starts_with("sk-") || lower.starts_with("key=")) && part.len() > 12);
        output.push(if secret { "[REDACTED]" } else { part });
        redact_next = lower == "bearer" || lower.ends_with("api_key:");
    }
    output.join(" ")
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

fn string_field(line: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let start = line.find(&marker)? + marker.len();
    read_json_string_content(&line[start..]).map(|(value, _)| value)
}

fn string_array_field(line: &str, key: &str) -> Option<Vec<String>> {
    let marker = format!("\"{key}\":[");
    let mut rest = &line[line.find(&marker)? + marker.len()..];
    let mut values = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(']') {
            return Some(values);
        }
        let content = rest.strip_prefix('"')?;
        let (value, used) = read_json_string_content(content)?;
        values.push(value);
        rest = &content[used + 1..];
        rest = rest.trim_start();
        if let Some(next) = rest.strip_prefix(',') {
            rest = next;
        } else if rest.starts_with(']') {
            return Some(values);
        } else {
            return None;
        }
    }
}

fn usize_field(line: &str, key: &str) -> Option<usize> {
    unsigned_field(line, key)?.try_into().ok()
}

fn u32_field(line: &str, key: &str) -> Option<u32> {
    unsigned_field(line, key)?.try_into().ok()
}

fn u64_field(line: &str, key: &str) -> Option<u64> {
    unsigned_field(line, key)
}

fn unsigned_field(line: &str, key: &str) -> Option<u64> {
    let marker = format!("\"{key}\":");
    let rest = &line[line.find(&marker)? + marker.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn read_json_string_content(input: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
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
            return Some((out, index));
        } else {
            out.push(ch);
        }
    }
    None
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
                "fnv:1234567890abcdef".to_string()
            } else {
                String::new()
            },
            provider_model: "deepseek-chat".to_string(),
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
        fs::remove_file(path).ok();
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
        fs::remove_file(path).ok();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 1"));
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
        input.failure =
            "provider rejected sk-12345678901234567890 Bearer abcdefghijklmnop".to_string();
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
            route_len: input.route_len,
        });
        assert!(!sanitized.failure.contains("sk-123"));
        assert!(!sanitized.failure.contains("abcdefghijklmnop"));
        assert_eq!(sanitized.failure.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn artifact_hash_is_stable() {
        let path = temp_path("hash");
        fs::write(&path, b"artifact").unwrap();
        let first = artifact_hash(&path).unwrap();
        let second = artifact_hash(&path).unwrap();
        fs::remove_file(path).ok();
        assert_eq!(first, second);
        assert!(valid_fnv_hash(&first));
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
