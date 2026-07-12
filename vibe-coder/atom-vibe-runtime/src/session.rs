use crate::model::{provider_identity, RuntimeError, SessionManifest, SESSION_SCHEMA_VERSION};
use atom_vibe_build_protocol::unix_time_ms;
use math_atoms_core::ProviderConfig;
use math_atoms_hash::{sha256_tagged, valid_sha256_tag};
use math_atoms_json::{parse, JsonValue};
use math_atoms_lock::acquire_file_lease;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
const MAX_OPERATOR_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        let store = Self { root: root.into() };
        fs::create_dir_all(&store.root).map_err(session_io)?;
        Ok(store)
    }

    pub fn create(
        &self,
        build_id: &str,
        project_id: &str,
        operator_request: &str,
        provider: &ProviderConfig,
    ) -> Result<SessionManifest, RuntimeError> {
        validate_identity(build_id, project_id, operator_request)?;
        let _lease = acquire_file_lease(
            self.root.join(format!("{build_id}.lock")),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(session_io)?;
        let provider_identity_hash = sha256_tagged(provider_identity(provider).as_bytes());
        let mut manifest = SessionManifest {
            schema_version: SESSION_SCHEMA_VERSION,
            build_id: build_id.to_string(),
            project_id: project_id.to_string(),
            operator_request: operator_request.to_string(),
            initial_provider: provider.kind.as_str().to_string(),
            initial_model: provider.model.clone(),
            initial_provider_identity_hash: provider_identity_hash,
            created_at_unix_ms: unix_time_ms(),
            manifest_hash: String::new(),
        };
        manifest.manifest_hash = manifest_hash(&manifest);
        validate_manifest(&manifest)?;
        let path = self.path(build_id);
        write_new_atomic(&path, manifest_json(&manifest).as_bytes()).map_err(session_io)?;
        let readback = self.load_unlocked(build_id)?;
        if readback != manifest {
            return Err(RuntimeError::Session(
                "session readback changed after creation".to_string(),
            ));
        }
        Ok(manifest)
    }

    pub fn load(&self, build_id: &str) -> Result<SessionManifest, RuntimeError> {
        let _lease = acquire_file_lease(
            self.root.join(format!("{build_id}.lock")),
            LOCK_TIMEOUT,
            STALE_LOCK_AGE,
        )
        .map_err(session_io)?;
        self.load_unlocked(build_id)
    }

    fn load_unlocked(&self, build_id: &str) -> Result<SessionManifest, RuntimeError> {
        let text = fs::read_to_string(self.path(build_id)).map_err(session_io)?;
        let manifest = parse_manifest(&text)?;
        if manifest.build_id != build_id {
            return Err(RuntimeError::Session(
                "session filename and build id differ".to_string(),
            ));
        }
        validate_manifest(&manifest)?;
        Ok(manifest)
    }

    fn path(&self, build_id: &str) -> PathBuf {
        self.root.join(format!("{build_id}.json"))
    }
}

fn validate_identity(
    build_id: &str,
    project_id: &str,
    operator_request: &str,
) -> Result<(), RuntimeError> {
    if !build_id.starts_with("build-")
        || build_id.len() > 160
        || !build_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(RuntimeError::Session("build id is unsafe".to_string()));
    }
    if project_id.trim().is_empty()
        || project_id.len() > 240
        || project_id.chars().any(char::is_control)
    {
        return Err(RuntimeError::InvalidRequest(
            "project id is empty, unsafe, or exceeds 240 bytes".to_string(),
        ));
    }
    if operator_request.trim().is_empty()
        || operator_request.len() > MAX_OPERATOR_REQUEST_BYTES
        || operator_request.chars().any(|ch| ch == '\0')
    {
        return Err(RuntimeError::InvalidRequest(format!(
            "operator request must be nonempty and no larger than {MAX_OPERATOR_REQUEST_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_manifest(manifest: &SessionManifest) -> Result<(), RuntimeError> {
    validate_identity(
        &manifest.build_id,
        &manifest.project_id,
        &manifest.operator_request,
    )?;
    if manifest.schema_version != SESSION_SCHEMA_VERSION
        || manifest.initial_provider.trim().is_empty()
        || manifest.initial_model.trim().is_empty()
        || !valid_sha256_tag(&manifest.initial_provider_identity_hash)
        || !valid_sha256_tag(&manifest.manifest_hash)
        || manifest.manifest_hash != manifest_hash(manifest)
    {
        return Err(RuntimeError::Session(
            "session manifest failed integrity validation".to_string(),
        ));
    }
    Ok(())
}

fn manifest_hash(manifest: &SessionManifest) -> String {
    sha256_tagged(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            manifest.schema_version,
            manifest.build_id,
            manifest.project_id,
            manifest.operator_request,
            manifest.initial_provider,
            manifest.initial_model,
            manifest.initial_provider_identity_hash,
            manifest.created_at_unix_ms
        )
        .as_bytes(),
    )
}

fn manifest_json(manifest: &SessionManifest) -> String {
    format!(
        "{{\"schema_version\":{},\"build_id\":\"{}\",\"project_id\":\"{}\",\"operator_request\":\"{}\",\"initial_provider\":\"{}\",\"initial_model\":\"{}\",\"initial_provider_identity_hash\":\"{}\",\"created_at_unix_ms\":{},\"manifest_hash\":\"{}\"}}",
        manifest.schema_version,
        json_escape(&manifest.build_id),
        json_escape(&manifest.project_id),
        json_escape(&manifest.operator_request),
        json_escape(&manifest.initial_provider),
        json_escape(&manifest.initial_model),
        json_escape(&manifest.initial_provider_identity_hash),
        manifest.created_at_unix_ms,
        json_escape(&manifest.manifest_hash)
    )
}

fn parse_manifest(text: &str) -> Result<SessionManifest, RuntimeError> {
    let root = parse(text).map_err(|error| RuntimeError::Session(error.to_string()))?;
    Ok(SessionManifest {
        schema_version: required_u64(&root, "schema_version")? as u32,
        build_id: required_string(&root, "build_id")?,
        project_id: required_string(&root, "project_id")?,
        operator_request: required_string(&root, "operator_request")?,
        initial_provider: required_string(&root, "initial_provider")?,
        initial_model: required_string(&root, "initial_model")?,
        initial_provider_identity_hash: required_string(&root, "initial_provider_identity_hash")?,
        created_at_unix_ms: required_u64(&root, "created_at_unix_ms")?,
        manifest_hash: required_string(&root, "manifest_hash")?,
    })
}

fn required_string(root: &JsonValue, key: &str) -> Result<String, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| RuntimeError::Session(format!("session field {key} is invalid")))
}

fn required_u64(root: &JsonValue, key: &str) -> Result<u64, RuntimeError> {
    root.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| RuntimeError::Session(format!("session field {key} is invalid")))
}

fn write_new_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("session path has no parent"))?;
    fs::create_dir_all(parent)?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("session"),
        std::process::id(),
        suffix
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        fs::remove_file(&temp)?;
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "session already exists",
        ));
    }
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

pub(crate) fn json_escape(value: &str) -> String {
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

fn session_io(error: std::io::Error) -> RuntimeError {
    RuntimeError::Session(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use math_atoms_core::ProviderConfig;

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "atom-vibe-session-{}-{}",
            std::process::id(),
            unix_time_ms()
        ))
    }

    #[test]
    fn session_round_trips_and_rejects_tampering() {
        let root = root();
        let store = SessionStore::open(&root).unwrap();
        let config = ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "configured")]);
        let build_id = "build-aaaaaaaaaaaaaaaaaaaaaaaa";
        let created = store
            .create(build_id, "inventory", "Build an inventory app", &config)
            .unwrap();
        assert_eq!(store.load(build_id).unwrap(), created);
        let path = root.join(format!("{build_id}.json"));
        let changed = fs::read_to_string(&path)
            .unwrap()
            .replace("inventory", "different");
        fs::write(&path, changed).unwrap();
        assert!(store.load(build_id).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
