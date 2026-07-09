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
        Ok(self
            .read_to_string()?
            .lines()
            .filter_map(ProofRecord::from_json)
            .collect())
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

    pub fn from_json(line: &str) -> Option<Self> {
        Some(Self {
            recipe_id: string_field(line, "recipe_id")?,
            status: string_field(line, "status")?,
            atoms: string_array_field(line, "atoms")?,
            evidence_count: usize_field(line, "evidence_count")?,
            blockers: string_array_field(line, "blockers")?,
            provider_state: string_field(line, "provider_state")?,
            route_len: usize_field(line, "route_len")?,
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
        if !rest.starts_with('"') {
            return None;
        }
        let (value, used) = read_json_string_content(&rest[1..])?;
        values.push(value);
        rest = &rest[used + 2..];
        rest = rest.trim_start();
        if rest.starts_with(',') {
            rest = &rest[1..];
        } else if rest.starts_with(']') {
            return Some(values);
        } else {
            return None;
        }
    }
}

fn usize_field(line: &str, key: &str) -> Option<usize> {
    let marker = format!("\"{key}\":");
    let rest = &line[line.find(&marker)? + marker.len()..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn read_json_string_content(input: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
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
            return Some((out, idx));
        } else {
            out.push(ch);
        }
    }
    None
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
            route_len: 5,
        };
        store.append(&record).unwrap();
        let records = store.read_records().unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(records, vec![record]);
    }
}
