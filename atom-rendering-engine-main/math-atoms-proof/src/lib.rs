//! Strict proof records and durable JSONL persistence.

use math_atoms_json::{parse as parse_json, JsonValue};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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
    pub route_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofStore {
    path: PathBuf,
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
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", record.to_json())?;
        file.flush()?;
        file.sync_data()
    }

    pub fn read_to_string(&self) -> io::Result<String> {
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

impl ProofRecord {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"recipe_id\":\"{}\",\"status\":\"{}\",\"atoms\":[{}],\"evidence_count\":{},\"blockers\":[{}],\"provider_state\":\"{}\",\"provider_model\":\"{}\",\"provider_endpoint\":\"{}\",\"provider_output_artifact\":\"{}\",\"provider_output_hash\":\"{}\",\"provider_output_len\":{},\"route_len\":{}}}",
            escape(&self.recipe_id),
            escape(&self.status),
            string_array(&self.atoms),
            self.evidence_count,
            string_array(&self.blockers),
            escape(&self.provider_state),
            escape(&self.provider_model),
            escape(&self.provider_endpoint),
            escape(&self.provider_output_artifact),
            escape(&self.provider_output_hash),
            self.provider_output_len,
            self.route_len
        )
    }

    pub fn from_json(line: &str) -> Option<Self> {
        const ALLOWED_FIELDS: [&str; 12] = [
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
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: "C:/audit/provider.txt".to_string(),
            provider_output_hash:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            provider_output_len: 18,
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
        assert_eq!(record.route_len, 5);
    }
}
