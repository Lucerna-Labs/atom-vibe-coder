use crate::model::{safe_relative, RuntimeError, TurnRecord, TURN_RECORD_SCHEMA_VERSION};
use crate::session::json_escape;
use atom_vibe_build_protocol::{unix_time_ms, BuildStep};
use atom_vibe_provider::ProviderTurnReceipt;
use math_atoms_hash::{sha256_file, sha256_tagged, valid_sha256_tag};
use math_atoms_json::{parse, JsonValue};
use math_atoms_lock::acquire_file_lease;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
const MAX_TURNS_PER_BUILD: usize = 4_096;

pub(crate) struct NewTurnRecord<'a> {
    pub build_id: &'a str,
    pub step: BuildStep,
    pub planner_revision: u64,
    pub receipt: &'a ProviderTurnReceipt,
    pub output_artifact: &'a str,
    pub evidence_ids: &'a [String],
    pub context_route: &'a [u64],
    pub result_route: &'a [u64],
    pub scratchpad_entry_hash: &'a str,
}

#[derive(Clone, Debug)]
pub(crate) struct TurnStore {
    root: PathBuf,
    state_root: PathBuf,
}

impl TurnStore {
    pub fn open(
        root: impl Into<PathBuf>,
        state_root: impl Into<PathBuf>,
    ) -> Result<Self, RuntimeError> {
        let store = Self {
            root: root.into(),
            state_root: state_root.into(),
        };
        fs::create_dir_all(&store.root).map_err(turn_io)?;
        Ok(store)
    }

    pub fn append(&self, input: NewTurnRecord<'_>) -> Result<TurnRecord, RuntimeError> {
        let _lease = acquire_file_lease(
            self.root.join(format!("{}.lock", input.build_id)),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(turn_io)?;
        let records = self.load_unlocked(input.build_id)?;
        if records.len() >= MAX_TURNS_PER_BUILD {
            return Err(RuntimeError::TurnStore(format!(
                "turn limit is {MAX_TURNS_PER_BUILD} per build"
            )));
        }
        let mut record = TurnRecord {
            schema_version: TURN_RECORD_SCHEMA_VERSION,
            ordinal: records.len() as u64 + 1,
            build_id: input.build_id.to_string(),
            step: input.step,
            planner_revision: input.planner_revision,
            provider: input.receipt.provider.clone(),
            model: input.receipt.model.clone(),
            request_body_hash: input.receipt.request_body_hash.clone(),
            raw_response_hash: input.receipt.raw_response_hash.clone(),
            output_hash: input.receipt.output_hash.clone(),
            output_artifact: input.output_artifact.to_string(),
            elapsed_ms: input.receipt.elapsed_ms.min(u128::from(u64::MAX)) as u64,
            input_tokens: input.receipt.usage.input_tokens,
            output_tokens: input.receipt.usage.output_tokens,
            reasoning_tokens: input.receipt.usage.reasoning_tokens,
            thinking_source: input.receipt.thinking.source.clone(),
            evidence_ids: input.evidence_ids.to_vec(),
            context_route: input.context_route.to_vec(),
            result_route: input.result_route.to_vec(),
            scratchpad_entry_hash: input.scratchpad_entry_hash.to_string(),
            previous_hash: records
                .last()
                .map(|record| record.record_hash.clone())
                .unwrap_or_default(),
            record_hash: String::new(),
            created_at_unix_ms: unix_time_ms(),
        };
        record.record_hash = record_hash(&record);
        validate_record(&record, &self.state_root, records.last())?;
        let path = self.record_path(&record);
        write_new_atomic(&path, record_json(&record).as_bytes()).map_err(turn_io)?;
        let readback = parse_record(&fs::read_to_string(&path).map_err(turn_io)?)?;
        validate_record(&readback, &self.state_root, records.last())?;
        if readback != record {
            return Err(RuntimeError::TurnStore(
                "turn record changed on readback".to_string(),
            ));
        }
        Ok(record)
    }

    pub fn load(&self, build_id: &str) -> Result<Vec<TurnRecord>, RuntimeError> {
        let _lease = acquire_file_lease(
            self.root.join(format!("{build_id}.lock")),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(turn_io)?;
        self.load_unlocked(build_id)
    }

    fn load_unlocked(&self, build_id: &str) -> Result<Vec<TurnRecord>, RuntimeError> {
        let dir = self.root.join(build_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(&dir)
            .map_err(turn_io)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        if paths.len() > MAX_TURNS_PER_BUILD {
            return Err(RuntimeError::TurnStore(
                "turn store exceeds its record limit".to_string(),
            ));
        }
        let mut records = Vec::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            let record = parse_record(&fs::read_to_string(path).map_err(turn_io)?)?;
            if record.build_id != build_id || record.ordinal != index as u64 + 1 {
                return Err(RuntimeError::TurnStore(
                    "turn record order or build identity is invalid".to_string(),
                ));
            }
            validate_record(&record, &self.state_root, records.last())?;
            if self.record_path(&record).file_name() != path.file_name() {
                return Err(RuntimeError::TurnStore(
                    "turn filename does not match record hash".to_string(),
                ));
            }
            records.push(record);
        }
        Ok(records)
    }

    fn record_path(&self, record: &TurnRecord) -> PathBuf {
        self.root.join(&record.build_id).join(format!(
            "{:08}-{}.json",
            record.ordinal,
            record.record_hash.trim_start_matches("sha256:")
        ))
    }
}

fn validate_record(
    record: &TurnRecord,
    state_root: &Path,
    previous: Option<&TurnRecord>,
) -> Result<(), RuntimeError> {
    let expected_previous = previous
        .map(|value| value.record_hash.as_str())
        .unwrap_or("");
    if record.schema_version != TURN_RECORD_SCHEMA_VERSION
        || record.ordinal == 0
        || record.build_id.trim().is_empty()
        || record.provider.trim().is_empty()
        || record.model.trim().is_empty()
        || record.thinking_source.trim().is_empty()
        || record.previous_hash != expected_previous
        || !valid_sha256_tag(&record.request_body_hash)
        || !valid_sha256_tag(&record.raw_response_hash)
        || !valid_sha256_tag(&record.output_hash)
        || !valid_sha256_tag(&record.scratchpad_entry_hash)
        || !valid_sha256_tag(&record.record_hash)
        || record.record_hash != record_hash(record)
        || record.context_route.len() != 4
        || record.result_route.len() != 4
    {
        return Err(RuntimeError::TurnStore(
            "turn record failed structural or hash validation".to_string(),
        ));
    }
    let relative = safe_relative(Path::new(&record.output_artifact)).ok_or_else(|| {
        RuntimeError::TurnStore("turn output artifact path is unsafe".to_string())
    })?;
    let artifact = state_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !artifact.is_file() || sha256_file(&artifact).map_err(turn_io)? != record.output_hash {
        return Err(RuntimeError::TurnStore(
            "turn output artifact is missing or changed".to_string(),
        ));
    }
    Ok(())
}

fn record_hash(record: &TurnRecord) -> String {
    let evidence = record.evidence_ids.join("\u{1f}");
    let context = record
        .context_route
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let result = record
        .result_route
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    sha256_tagged(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{:?}\0{:?}\0{:?}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            record.schema_version,
            record.ordinal,
            record.build_id,
            record.step.as_str(),
            record.planner_revision,
            record.provider,
            record.model,
            record.request_body_hash,
            record.raw_response_hash,
            record.output_hash,
            record.output_artifact,
            record.elapsed_ms,
            record.input_tokens,
            record.output_tokens,
            record.reasoning_tokens,
            record.thinking_source,
            evidence,
            context,
            result,
            record.scratchpad_entry_hash,
            record.previous_hash,
            record.created_at_unix_ms
        )
        .as_bytes(),
    )
}

fn record_json(record: &TurnRecord) -> String {
    format!(
        "{{\"schema_version\":{},\"ordinal\":{},\"build_id\":\"{}\",\"step\":\"{}\",\"planner_revision\":{},\"provider\":\"{}\",\"model\":\"{}\",\"request_body_hash\":\"{}\",\"raw_response_hash\":\"{}\",\"output_hash\":\"{}\",\"output_artifact\":\"{}\",\"elapsed_ms\":{},\"input_tokens\":{},\"output_tokens\":{},\"reasoning_tokens\":{},\"thinking_source\":\"{}\",\"evidence_ids\":{},\"context_route\":{},\"result_route\":{},\"scratchpad_entry_hash\":\"{}\",\"previous_hash\":\"{}\",\"record_hash\":\"{}\",\"created_at_unix_ms\":{}}}",
        record.schema_version,
        record.ordinal,
        json_escape(&record.build_id),
        record.step.as_str(),
        record.planner_revision,
        json_escape(&record.provider),
        json_escape(&record.model),
        json_escape(&record.request_body_hash),
        json_escape(&record.raw_response_hash),
        json_escape(&record.output_hash),
        json_escape(&record.output_artifact),
        record.elapsed_ms,
        optional_u64(record.input_tokens),
        optional_u64(record.output_tokens),
        optional_u64(record.reasoning_tokens),
        json_escape(&record.thinking_source),
        string_array_json(&record.evidence_ids),
        u64_array_json(&record.context_route),
        u64_array_json(&record.result_route),
        json_escape(&record.scratchpad_entry_hash),
        json_escape(&record.previous_hash),
        json_escape(&record.record_hash),
        record.created_at_unix_ms
    )
}

fn parse_record(text: &str) -> Result<TurnRecord, RuntimeError> {
    let root = parse(text).map_err(|error| RuntimeError::TurnStore(error.to_string()))?;
    Ok(TurnRecord {
        schema_version: required_u64(&root, "schema_version")? as u32,
        ordinal: required_u64(&root, "ordinal")?,
        build_id: required_string(&root, "build_id")?,
        step: BuildStep::parse(&required_string(&root, "step")?)
            .ok_or_else(|| RuntimeError::TurnStore("turn step is invalid".to_string()))?,
        planner_revision: required_u64(&root, "planner_revision")?,
        provider: required_string(&root, "provider")?,
        model: required_string(&root, "model")?,
        request_body_hash: required_string(&root, "request_body_hash")?,
        raw_response_hash: required_string(&root, "raw_response_hash")?,
        output_hash: required_string(&root, "output_hash")?,
        output_artifact: required_string(&root, "output_artifact")?,
        elapsed_ms: required_u64(&root, "elapsed_ms")?,
        input_tokens: optional_number(&root, "input_tokens")?,
        output_tokens: optional_number(&root, "output_tokens")?,
        reasoning_tokens: optional_number(&root, "reasoning_tokens")?,
        thinking_source: required_string(&root, "thinking_source")?,
        evidence_ids: string_array(&root, "evidence_ids")?,
        context_route: u64_array(&root, "context_route")?,
        result_route: u64_array(&root, "result_route")?,
        scratchpad_entry_hash: required_string(&root, "scratchpad_entry_hash")?,
        previous_hash: required_string(&root, "previous_hash")?,
        record_hash: required_string(&root, "record_hash")?,
        created_at_unix_ms: required_u64(&root, "created_at_unix_ms")?,
    })
}

fn required_string(root: &JsonValue, key: &str) -> Result<String, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))
}

fn required_u64(root: &JsonValue, key: &str) -> Result<u64, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))
}

fn optional_number(root: &JsonValue, key: &str) -> Result<Option<u64>, RuntimeError> {
    match root.get(key) {
        Some(JsonValue::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid"))),
        None => Err(RuntimeError::TurnStore(format!(
            "turn field {key} is missing"
        ))),
    }
}

fn string_array(root: &JsonValue, key: &str) -> Result<Vec<String>, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))
        })
        .collect()
}

fn u64_array(root: &JsonValue, key: &str) -> Result<Vec<u64>, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| RuntimeError::TurnStore(format!("turn field {key} is invalid")))
        })
        .collect()
}

fn optional_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn string_array_json(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn u64_array_json(values: &[u64]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("turn path has no parent"))?;
    fs::create_dir_all(parent)?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".turn.{}.{}.tmp", std::process::id(), suffix));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

fn turn_io(error: std::io::Error) -> RuntimeError {
    RuntimeError::TurnStore(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_vibe_provider::{ThinkingEvidence, TokenUsage};

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-turn-store-{}-{}",
            std::process::id(),
            unix_time_ms()
        ))
    }

    #[test]
    fn turn_chain_recomputes_output_and_rejects_tampering() {
        let root = root();
        let output_dir = root.join("outputs/build-test");
        fs::create_dir_all(&output_dir).unwrap();
        let output = output_dir.join("answer.txt");
        fs::write(&output, "answer").unwrap();
        let output_hash = sha256_file(&output).unwrap();
        let store = TurnStore::open(root.join("turns"), &root).unwrap();
        let receipt = ProviderTurnReceipt {
            request_id: "turn-1".to_string(),
            provider: "custom".to_string(),
            model: "qwen3.5-9b-q8".to_string(),
            text: "answer".to_string(),
            request_body_hash: sha256_tagged(b"request"),
            raw_response_hash: sha256_tagged(b"response"),
            output_hash,
            elapsed_ms: 4,
            usage: TokenUsage::default(),
            thinking: ThinkingEvidence {
                source: "reasoning".to_string(),
                reasoning_tokens: None,
            },
        };
        let record = store
            .append(NewTurnRecord {
                build_id: "build-test",
                step: BuildStep::Intake,
                planner_revision: 1,
                receipt: &receipt,
                output_artifact: "outputs/build-test/answer.txt",
                evidence_ids: &["wiki:thinking-model-requirements".to_string()],
                context_route: &[1, 2, 3, 4],
                result_route: &[5, 6, 7, 8],
                scratchpad_entry_hash: &sha256_tagged(b"scratchpad"),
            })
            .unwrap();
        assert_eq!(store.load("build-test").unwrap(), vec![record]);
        fs::write(output, "changed").unwrap();
        assert!(store.load("build-test").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
