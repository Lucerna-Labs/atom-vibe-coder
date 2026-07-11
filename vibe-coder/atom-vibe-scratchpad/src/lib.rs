//! Resumable working context for one build and one provider-model identity.
//!
//! Scratchpads are deliberately not memory. This crate has no graph or learning
//! dependency, never performs retrieval across scopes, and exposes only explicit
//! scope-bound load, append, projection, and seal operations.

use atom_vibe_build_protocol::{unix_time_ms, BuildStep};
use math_atoms_hash::{sha256_tagged, valid_sha256_tag};
use math_atoms_json::{parse, JsonValue};
use math_atoms_lock::acquire_file_lease;
use math_atoms_secrets::redact_sensitive_text;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SCRATCHPAD_SCHEMA_VERSION: u32 = 1;
pub const MAX_SCRATCHPAD_ENTRIES: usize = 512;
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const MAX_SCRATCHPAD_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROJECTION_BYTES: usize = 32 * 1024;

const MAX_SOURCE_IDS: usize = 64;
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchpadScope {
    pub build_id: String,
    pub model_scope_hash: String,
}

impl ScratchpadScope {
    pub fn new(
        build_id: impl Into<String>,
        provider_model_identity: &str,
    ) -> Result<Self, ScratchpadError> {
        let build_id = build_id.into();
        validate_build_id(&build_id)?;
        if provider_model_identity.trim().is_empty() || provider_model_identity.len() > 8 * 1024 {
            return Err(ScratchpadError::InvalidScope(
                "provider-model identity is empty or exceeds bounds".to_string(),
            ));
        }
        Ok(Self {
            build_id,
            model_scope_hash: sha256_tagged(provider_model_identity.as_bytes()),
        })
    }

    pub fn validate(&self) -> Result<(), ScratchpadError> {
        validate_build_id(&self.build_id)?;
        if !valid_sha256_tag(&self.model_scope_hash) {
            return Err(ScratchpadError::InvalidScope(
                "model scope is not a SHA-256 identity".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScratchpadEntryKind {
    Observation,
    Decision,
    PacketOutput,
    GateFailure,
    Correction,
    Checkpoint,
}

impl ScratchpadEntryKind {
    pub const ALL: [Self; 6] = [
        Self::Observation,
        Self::Decision,
        Self::PacketOutput,
        Self::GateFailure,
        Self::Correction,
        Self::Checkpoint,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Decision => "decision",
            Self::PacketOutput => "packet_output",
            Self::GateFailure => "gate_failure",
            Self::Correction => "correction",
            Self::Checkpoint => "checkpoint",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchpadEntry {
    pub schema_version: u32,
    pub ordinal: u64,
    pub build_id: String,
    pub model_scope_hash: String,
    pub step: Option<BuildStep>,
    pub kind: ScratchpadEntryKind,
    pub content: String,
    pub content_hash: String,
    pub previous_hash: String,
    pub entry_hash: String,
    pub source_ids: Vec<String>,
    pub created_at_unix_ms: u64,
}

impl ScratchpadEntry {
    fn create(
        scope: &ScratchpadScope,
        ordinal: u64,
        previous_hash: String,
        step: Option<BuildStep>,
        kind: ScratchpadEntryKind,
        content: String,
        source_ids: Vec<String>,
    ) -> Result<Self, ScratchpadError> {
        validate_content(&content)?;
        validate_source_ids(&source_ids)?;
        let content_hash = sha256_tagged(content.as_bytes());
        let created_at_unix_ms = unix_time_ms();
        let entry_hash = compute_entry_hash(
            scope,
            EntryHashInput {
                ordinal,
                previous_hash: &previous_hash,
                step,
                kind,
                content_hash: &content_hash,
                source_ids: &source_ids,
                created_at_unix_ms,
            },
        );
        Ok(Self {
            schema_version: SCRATCHPAD_SCHEMA_VERSION,
            ordinal,
            build_id: scope.build_id.clone(),
            model_scope_hash: scope.model_scope_hash.clone(),
            step,
            kind,
            content,
            content_hash,
            previous_hash,
            entry_hash,
            source_ids,
            created_at_unix_ms,
        })
    }

    fn validate(
        &self,
        scope: &ScratchpadScope,
        expected_ordinal: u64,
        expected_previous_hash: &str,
    ) -> Result<(), ScratchpadError> {
        if self.schema_version != SCRATCHPAD_SCHEMA_VERSION
            || self.ordinal != expected_ordinal
            || self.build_id != scope.build_id
            || self.model_scope_hash != scope.model_scope_hash
            || self.previous_hash != expected_previous_hash
        {
            return Err(ScratchpadError::Evidence(format!(
                "scratchpad chain header is invalid at entry {}",
                self.ordinal
            )));
        }
        validate_content(&self.content)?;
        validate_source_ids(&self.source_ids)?;
        if self.content_hash != sha256_tagged(self.content.as_bytes()) {
            return Err(ScratchpadError::Evidence(format!(
                "scratchpad content hash failed at entry {}",
                self.ordinal
            )));
        }
        let expected = compute_entry_hash(
            scope,
            EntryHashInput {
                ordinal: self.ordinal,
                previous_hash: &self.previous_hash,
                step: self.step,
                kind: self.kind,
                content_hash: &self.content_hash,
                source_ids: &self.source_ids,
                created_at_unix_ms: self.created_at_unix_ms,
            },
        );
        if self.entry_hash != expected {
            return Err(ScratchpadError::Evidence(format!(
                "scratchpad entry hash failed at entry {}",
                self.ordinal
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScratchpadProjection {
    pub build_id: String,
    pub model_scope_hash: String,
    pub step: BuildStep,
    pub entry_count: usize,
    pub last_entry_hash: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct ScratchpadStore {
    root: PathBuf,
    scope: ScratchpadScope,
}

impl ScratchpadStore {
    pub fn open(root: impl Into<PathBuf>, scope: ScratchpadScope) -> Result<Self, ScratchpadError> {
        scope.validate()?;
        let store = Self {
            root: root.into(),
            scope,
        };
        fs::create_dir_all(store.entries_dir())?;
        let _lease = store.acquire_lease()?;
        store.write_or_verify_scope()?;
        store.load_unlocked()?;
        Ok(store)
    }

    pub fn default_for(scope: ScratchpadScope) -> Result<Self, ScratchpadError> {
        Self::open(default_scratchpad_root(), scope)
    }

    pub fn scope(&self) -> &ScratchpadScope {
        &self.scope
    }

    pub fn scope_dir(&self) -> PathBuf {
        self.root
            .join(&self.scope.build_id)
            .join(self.scope.model_scope_hash.trim_start_matches("sha256:"))
    }

    pub fn append(
        &self,
        step: Option<BuildStep>,
        kind: ScratchpadEntryKind,
        content: &str,
        source_ids: &[String],
    ) -> Result<ScratchpadEntry, ScratchpadError> {
        let _lease = self.acquire_lease()?;
        if self.seal_path().exists() {
            return Err(ScratchpadError::Sealed);
        }
        let entries = self.load_unlocked()?;
        if entries.len() >= MAX_SCRATCHPAD_ENTRIES {
            return Err(ScratchpadError::LimitExceeded(format!(
                "scratchpad entry limit is {MAX_SCRATCHPAD_ENTRIES}"
            )));
        }
        let redacted = redact_sensitive_text(content);
        let projected_bytes = entries
            .iter()
            .map(|entry| entry.content.len())
            .sum::<usize>()
            .saturating_add(redacted.len());
        if projected_bytes > MAX_SCRATCHPAD_BYTES {
            return Err(ScratchpadError::LimitExceeded(format!(
                "scratchpad content limit is {MAX_SCRATCHPAD_BYTES} bytes"
            )));
        }
        let ordinal = entries.len() as u64 + 1;
        let previous_hash = entries
            .last()
            .map(|entry| entry.entry_hash.clone())
            .unwrap_or_default();
        let entry = ScratchpadEntry::create(
            &self.scope,
            ordinal,
            previous_hash,
            step,
            kind,
            redacted,
            source_ids.to_vec(),
        )?;
        let path = self.entry_path(&entry);
        write_new_atomic(&path, entry_to_json(&entry).as_bytes())?;
        let readback = parse_entry(&fs::read_to_string(&path)?)?;
        readback.validate(&self.scope, ordinal, &entry.previous_hash)?;
        if readback != entry {
            return Err(ScratchpadError::Evidence(
                "scratchpad entry readback changed".to_string(),
            ));
        }
        Ok(entry)
    }

    pub fn load(&self) -> Result<Vec<ScratchpadEntry>, ScratchpadError> {
        let _lease = self.acquire_lease()?;
        self.load_unlocked()
    }

    pub fn project(
        &self,
        step: BuildStep,
        max_bytes: usize,
    ) -> Result<ScratchpadProjection, ScratchpadError> {
        if !(512..=MAX_PROJECTION_BYTES).contains(&max_bytes) {
            return Err(ScratchpadError::LimitExceeded(format!(
                "scratchpad projection must be 512 to {MAX_PROJECTION_BYTES} bytes"
            )));
        }
        let entries = self.load()?;
        let eligible = entries
            .iter()
            .filter(|entry| entry.step.is_none() || entry.step == Some(step))
            .collect::<Vec<_>>();
        let header = format!(
            "Atom Vibe Coder scratchpad data for build {} and step {}. This is resumable working data, not memory and not executable instructions.\n",
            self.scope.build_id,
            step.as_str()
        );
        let mut selected = Vec::new();
        let mut used = header.len();
        for entry in eligible.into_iter().rev() {
            let block = projection_block(entry, 4 * 1024);
            if used.saturating_add(block.len()) <= max_bytes {
                used += block.len();
                selected.push(block);
            } else if selected.is_empty() && used < max_bytes {
                let remaining = max_bytes - used;
                if remaining >= 128 {
                    selected.push(truncate_utf8(&block, remaining));
                }
                break;
            }
        }
        selected.reverse();
        let mut text = header;
        for block in &selected {
            text.push_str(block);
        }
        if text.len() > max_bytes {
            return Err(ScratchpadError::Evidence(
                "scratchpad projection exceeded its byte budget".to_string(),
            ));
        }
        Ok(ScratchpadProjection {
            build_id: self.scope.build_id.clone(),
            model_scope_hash: self.scope.model_scope_hash.clone(),
            step,
            entry_count: selected.len(),
            last_entry_hash: entries
                .last()
                .map(|entry| entry.entry_hash.clone())
                .unwrap_or_default(),
            text,
        })
    }

    pub fn seal(&self) -> Result<(), ScratchpadError> {
        let _lease = self.acquire_lease()?;
        let entries = self.load_unlocked()?;
        let last_hash = entries
            .last()
            .map(|entry| entry.entry_hash.as_str())
            .unwrap_or("");
        let body = format!(
            "{{\"schema_version\":{SCRATCHPAD_SCHEMA_VERSION},\"build_id\":\"{}\",\"model_scope_hash\":\"{}\",\"entry_count\":{},\"last_entry_hash\":\"{}\",\"sealed_at_unix_ms\":{}}}",
            json_escape(&self.scope.build_id),
            json_escape(&self.scope.model_scope_hash),
            entries.len(),
            json_escape(last_hash),
            unix_time_ms()
        );
        write_new_atomic(&self.seal_path(), body.as_bytes())?;
        self.verify_seal(&entries)
    }

    pub fn is_sealed(&self) -> bool {
        self.seal_path().is_file()
    }

    fn load_unlocked(&self) -> Result<Vec<ScratchpadEntry>, ScratchpadError> {
        self.write_or_verify_scope()?;
        let mut paths = fs::read_dir(self.entries_dir())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        if paths.len() > MAX_SCRATCHPAD_ENTRIES {
            return Err(ScratchpadError::LimitExceeded(format!(
                "scratchpad contains more than {MAX_SCRATCHPAD_ENTRIES} entries"
            )));
        }
        let mut entries = Vec::with_capacity(paths.len());
        let mut previous_hash = String::new();
        let mut total_bytes = 0usize;
        for (index, path) in paths.iter().enumerate() {
            let entry = parse_entry(&fs::read_to_string(path)?)?;
            entry.validate(&self.scope, index as u64 + 1, &previous_hash)?;
            let expected_name = self
                .entry_path(&entry)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_string();
            if path.file_name().and_then(|value| value.to_str()) != Some(expected_name.as_str()) {
                return Err(ScratchpadError::Evidence(format!(
                    "scratchpad filename does not match entry {}",
                    entry.ordinal
                )));
            }
            total_bytes = total_bytes.saturating_add(entry.content.len());
            if total_bytes > MAX_SCRATCHPAD_BYTES {
                return Err(ScratchpadError::LimitExceeded(format!(
                    "scratchpad content exceeds {MAX_SCRATCHPAD_BYTES} bytes"
                )));
            }
            previous_hash = entry.entry_hash.clone();
            entries.push(entry);
        }
        if self.seal_path().exists() {
            self.verify_seal(&entries)?;
        }
        Ok(entries)
    }

    fn write_or_verify_scope(&self) -> Result<(), ScratchpadError> {
        let body = format!(
            "{{\"schema_version\":{SCRATCHPAD_SCHEMA_VERSION},\"build_id\":\"{}\",\"model_scope_hash\":\"{}\"}}",
            json_escape(&self.scope.build_id),
            json_escape(&self.scope.model_scope_hash)
        );
        let path = self.scope_dir().join("scope.json");
        if path.exists() {
            if fs::read_to_string(path)? != body {
                return Err(ScratchpadError::Evidence(
                    "scratchpad scope manifest changed".to_string(),
                ));
            }
        } else {
            write_new_atomic(&path, body.as_bytes())?;
        }
        Ok(())
    }

    fn verify_seal(&self, entries: &[ScratchpadEntry]) -> Result<(), ScratchpadError> {
        let root = parse(&fs::read_to_string(self.seal_path())?)
            .map_err(|error| ScratchpadError::Json(error.to_string()))?;
        let count = number_field(&root, "entry_count")? as usize;
        let build_id = string_field(&root, "build_id")?;
        let model_scope_hash = string_field(&root, "model_scope_hash")?;
        let last_hash = string_field(&root, "last_entry_hash")?;
        let expected_last = entries
            .last()
            .map(|entry| entry.entry_hash.as_str())
            .unwrap_or("");
        if count != entries.len()
            || build_id != self.scope.build_id
            || model_scope_hash != self.scope.model_scope_hash
            || last_hash != expected_last
        {
            return Err(ScratchpadError::Evidence(
                "scratchpad seal does not match the entry chain".to_string(),
            ));
        }
        Ok(())
    }

    fn entries_dir(&self) -> PathBuf {
        self.scope_dir().join("entries")
    }

    fn entry_path(&self, entry: &ScratchpadEntry) -> PathBuf {
        self.entries_dir().join(format!(
            "{:06}-{}.json",
            entry.ordinal,
            &entry.entry_hash["sha256:".len().."sha256:".len() + 12]
        ))
    }

    fn seal_path(&self) -> PathBuf {
        self.scope_dir().join("seal.json")
    }

    fn acquire_lease(&self) -> Result<math_atoms_lock::FileLease, ScratchpadError> {
        acquire_file_lease(
            self.scope_dir().join("scratchpad.lock"),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(ScratchpadError::from)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScratchpadError {
    InvalidScope(String),
    InvalidEntry(String),
    LimitExceeded(String),
    Evidence(String),
    Sealed,
    Json(String),
    Io(String),
}

impl fmt::Display for ScratchpadError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScope(reason) => write!(output, "invalid scratchpad scope: {reason}"),
            Self::InvalidEntry(reason) => write!(output, "invalid scratchpad entry: {reason}"),
            Self::LimitExceeded(reason) => write!(output, "scratchpad limit exceeded: {reason}"),
            Self::Evidence(reason) => write!(output, "scratchpad evidence failed: {reason}"),
            Self::Sealed => output.write_str("scratchpad is sealed"),
            Self::Json(reason) => write!(output, "scratchpad JSON failed: {reason}"),
            Self::Io(reason) => write!(output, "scratchpad I/O failed: {reason}"),
        }
    }
}

impl std::error::Error for ScratchpadError {}

impl From<std::io::Error> for ScratchpadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(redact_sensitive_text(&error.to_string()))
    }
}

pub fn default_scratchpad_root() -> PathBuf {
    if let Some(path) = non_empty_env("MATH_ATOMS_SCRATCHPAD_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = non_empty_env("MATH_ATOMS_STORE_DIR") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("scratchpads");
    }
    if let Some(path) = non_empty_env("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("scratchpads");
    }
    std::env::temp_dir()
        .join("MathAtomsCoder")
        .join("scratchpads")
}

struct EntryHashInput<'a> {
    ordinal: u64,
    previous_hash: &'a str,
    step: Option<BuildStep>,
    kind: ScratchpadEntryKind,
    content_hash: &'a str,
    source_ids: &'a [String],
    created_at_unix_ms: u64,
}

fn compute_entry_hash(scope: &ScratchpadScope, input: EntryHashInput<'_>) -> String {
    let fields = [
        SCRATCHPAD_SCHEMA_VERSION.to_string(),
        scope.build_id.clone(),
        scope.model_scope_hash.clone(),
        input.ordinal.to_string(),
        input.previous_hash.to_string(),
        input.step.map(BuildStep::as_str).unwrap_or("").to_string(),
        input.kind.as_str().to_string(),
        input.content_hash.to_string(),
        encode_sequence(input.source_ids),
        input.created_at_unix_ms.to_string(),
    ];
    sha256_tagged(encode_sequence(&fields).as_bytes())
}

fn entry_to_json(entry: &ScratchpadEntry) -> String {
    let step = entry
        .step
        .map(|value| format!("\"{}\"", value.as_str()))
        .unwrap_or_else(|| "null".to_string());
    let sources = entry
        .source_ids
        .iter()
        .map(|value| format!("\"{}\"", json_escape(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"ordinal\":{},\"build_id\":\"{}\",\"model_scope_hash\":\"{}\",\"step\":{},\"kind\":\"{}\",\"content\":\"{}\",\"content_hash\":\"{}\",\"previous_hash\":\"{}\",\"entry_hash\":\"{}\",\"source_ids\":[{}],\"created_at_unix_ms\":{}}}",
        entry.schema_version,
        entry.ordinal,
        json_escape(&entry.build_id),
        json_escape(&entry.model_scope_hash),
        step,
        entry.kind.as_str(),
        json_escape(&entry.content),
        json_escape(&entry.content_hash),
        json_escape(&entry.previous_hash),
        json_escape(&entry.entry_hash),
        sources,
        entry.created_at_unix_ms
    )
}

fn parse_entry(input: &str) -> Result<ScratchpadEntry, ScratchpadError> {
    let root = parse(input).map_err(|error| ScratchpadError::Json(error.to_string()))?;
    let step = match root.get("step") {
        Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) => Some(BuildStep::parse(value).ok_or_else(|| {
            ScratchpadError::InvalidEntry(format!("unknown build step: {value}"))
        })?),
        _ => {
            return Err(ScratchpadError::InvalidEntry(
                "entry step must be a string or null".to_string(),
            ))
        }
    };
    let kind_text = string_field(&root, "kind")?;
    let kind = ScratchpadEntryKind::parse(&kind_text).ok_or_else(|| {
        ScratchpadError::InvalidEntry(format!("unknown scratchpad kind: {kind_text}"))
    })?;
    let source_ids = root
        .get("source_ids")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| ScratchpadError::InvalidEntry("source_ids must be an array".to_string()))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                ScratchpadError::InvalidEntry("source id must be a string".to_string())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ScratchpadEntry {
        schema_version: number_field(&root, "schema_version")? as u32,
        ordinal: number_field(&root, "ordinal")?,
        build_id: string_field(&root, "build_id")?,
        model_scope_hash: string_field(&root, "model_scope_hash")?,
        step,
        kind,
        content: string_field(&root, "content")?,
        content_hash: string_field(&root, "content_hash")?,
        previous_hash: string_field(&root, "previous_hash")?,
        entry_hash: string_field(&root, "entry_hash")?,
        source_ids,
        created_at_unix_ms: number_field(&root, "created_at_unix_ms")?,
    })
}

fn string_field(root: &JsonValue, key: &str) -> Result<String, ScratchpadError> {
    root.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| ScratchpadError::InvalidEntry(format!("missing string field {key}")))
}

fn number_field(root: &JsonValue, key: &str) -> Result<u64, ScratchpadError> {
    root.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| ScratchpadError::InvalidEntry(format!("missing number field {key}")))
}

fn validate_build_id(value: &str) -> Result<(), ScratchpadError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ScratchpadError::InvalidScope(format!(
            "unsafe build id: {value}"
        )));
    }
    Ok(())
}

fn validate_content(value: &str) -> Result<(), ScratchpadError> {
    if value.trim().is_empty() || value.len() > MAX_ENTRY_BYTES {
        return Err(ScratchpadError::InvalidEntry(format!(
            "content must be nonempty and no larger than {MAX_ENTRY_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_source_ids(values: &[String]) -> Result<(), ScratchpadError> {
    if values.len() > MAX_SOURCE_IDS
        || values.iter().any(|value| {
            value.trim() != value
                || value.is_empty()
                || value.len() > 512
                || value.chars().any(|ch| ch.is_control())
        })
    {
        return Err(ScratchpadError::InvalidEntry(format!(
            "source ids must contain at most {MAX_SOURCE_IDS} bounded values"
        )));
    }
    Ok(())
}

fn projection_block(entry: &ScratchpadEntry, content_limit: usize) -> String {
    format!(
        "\n--- scratchpad entry {} | {} | {} | {} ---\n{}\n--- end scratchpad entry {} ---\n",
        entry.ordinal,
        entry.kind.as_str(),
        entry.step.map(BuildStep::as_str).unwrap_or("all_steps"),
        entry.entry_hash,
        truncate_utf8(&entry.content, content_limit),
        entry.ordinal
    )
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> Result<(), ScratchpadError> {
    if path.exists() {
        return if fs::read(path)? == bytes {
            Ok(())
        } else {
            Err(ScratchpadError::Evidence(format!(
                "immutable scratchpad conflict at {}",
                path.display()
            )))
        };
    }
    let parent = path
        .parent()
        .ok_or_else(|| ScratchpadError::Io("scratchpad path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".scratchpad-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    if fs::read(path)? != bytes {
        return Err(ScratchpadError::Evidence(format!(
            "scratchpad readback failed at {}",
            path.display()
        )));
    }
    Ok(())
}

fn encode_sequence(values: &[String]) -> String {
    let mut output = String::new();
    for value in values {
        output.push_str(&value.len().to_string());
        output.push(':');
        output.push_str(value);
    }
    output
}

fn json_escape(value: &str) -> String {
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

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let suffix = "\n[scratchpad entry truncated]";
    if max_bytes <= suffix.len() {
        return String::new();
    }
    let mut end = (max_bytes - suffix.len()).min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &value[..end])
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-scratchpad-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn store(root: &Path, model: &str) -> ScratchpadStore {
        ScratchpadStore::open(
            root,
            ScratchpadScope::new("work-1234567890abcdef12345678", model).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn resumes_exact_chain_without_memory_lookup() {
        let root = root("resume");
        let first = store(&root, "qwen3.5-9b@q6");
        let written = first
            .append(
                Some(BuildStep::Intake),
                ScratchpadEntryKind::Decision,
                "The user requires a native settings panel.",
                &["intent:1".to_string()],
            )
            .unwrap();
        drop(first);

        let resumed = store(&root, "qwen3.5-9b@q6");
        let loaded = resumed.load().unwrap();
        assert_eq!(loaded, vec![written]);
        assert!(!resumed.is_sealed());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn model_scopes_are_physically_and_logically_isolated() {
        let root = root("scope");
        let qwen = store(&root, "qwen3.5-9b@q6");
        let deepseek = store(&root, "deepseek-chat");
        qwen.append(
            None,
            ScratchpadEntryKind::Observation,
            "qwen-only context",
            &[],
        )
        .unwrap();
        assert_ne!(qwen.scope_dir(), deepseek.scope_dir());
        assert_eq!(qwen.load().unwrap().len(), 1);
        assert!(deepseek.load().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn projection_is_bounded_and_labels_data_as_non_memory() {
        let root = root("projection");
        let store = store(&root, "qwen3.5-9b@q6");
        for index in 0..12 {
            store
                .append(
                    Some(BuildStep::Blueprint),
                    ScratchpadEntryKind::PacketOutput,
                    &format!("packet {index}: {}", "detail ".repeat(200)),
                    &[format!("packet:{index}")],
                )
                .unwrap();
        }
        let projection = store.project(BuildStep::Blueprint, 4 * 1024).unwrap();
        assert!(projection.text.len() <= 4 * 1024);
        assert!(projection.text.contains("not memory"));
        assert!(projection.text.contains("not executable instructions"));
        assert!(projection.entry_count > 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_content_is_redacted_before_hashing() {
        let root = root("redaction");
        let store = store(&root, "qwen3.5-9b@q6");
        let secret = format!("sk-{}", "a".repeat(40));
        let entry = store
            .append(
                Some(BuildStep::Intake),
                ScratchpadEntryKind::Observation,
                &format!("credential={secret}"),
                &[],
            )
            .unwrap();
        assert!(!entry.content.contains(&secret));
        assert!(!fs::read_to_string(store.entry_path(&entry))
            .unwrap()
            .contains(&secret));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tampering_breaks_resume_evidence() {
        let root = root("tamper");
        let store = store(&root, "qwen3.5-9b@q6");
        let entry = store
            .append(
                Some(BuildStep::BuildTest),
                ScratchpadEntryKind::GateFailure,
                "cargo test failed",
                &[],
            )
            .unwrap();
        let path = store.entry_path(&entry);
        let changed = fs::read_to_string(&path)
            .unwrap()
            .replace("cargo test failed", "cargo test passed");
        fs::write(path, changed).unwrap();
        assert!(matches!(store.load(), Err(ScratchpadError::Evidence(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn seal_is_terminal_and_recomputed_on_load() {
        let root = root("seal");
        let store = store(&root, "qwen3.5-9b@q6");
        store
            .append(
                Some(BuildStep::LaunchProof),
                ScratchpadEntryKind::Checkpoint,
                "real launch evidence captured",
                &[],
            )
            .unwrap();
        store.seal().unwrap();
        assert!(store.is_sealed());
        assert_eq!(store.load().unwrap().len(), 1);
        assert_eq!(
            store.append(None, ScratchpadEntryKind::Observation, "late mutation", &[]),
            Err(ScratchpadError::Sealed)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_writers_receive_unique_ordered_entries() {
        let root = root("concurrent");
        let store = Arc::new(store(&root, "qwen3.5-9b@q6"));
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|index| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    store
                        .append(
                            Some(BuildStep::CrateBuild),
                            ScratchpadEntryKind::PacketOutput,
                            &format!("writer {index}"),
                            &[format!("writer:{index}")],
                        )
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 8);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
