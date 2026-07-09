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
        writeln!(file, "{}", record.to_json())
    }

    pub fn read_to_string(&self) -> io::Result<String> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }
}

impl ProofRecord {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"recipe_id\":\"{}\",\"status\":\"{}\",\"atoms\":[{}],\"evidence_count\":{},\"blockers\":[{}],\"provider_state\":\"{}\",\"route_len\":{}}}",
            escape(&self.recipe_id),
            escape(&self.status),
            string_array(&self.atoms),
            self.evidence_count,
            string_array(&self.blockers),
            escape(&self.provider_state),
            self.route_len
        )
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
            recipe_id: "ornith-parity-runtime".to_string(),
            status: "proven".to_string(),
            atoms: vec!["flow".to_string(), "measure".to_string()],
            evidence_count: 3,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            route_len: 4,
        };
        store.append(&record).unwrap();
        let text = store.read_to_string().unwrap();
        fs::remove_file(&path).ok();
        assert!(text.contains("\"recipe_id\":\"ornith-parity-runtime\""));
        assert!(text.contains("\"provider_state\":\"provider:ran\""));
        assert!(text.ends_with('\n'));
    }
}
