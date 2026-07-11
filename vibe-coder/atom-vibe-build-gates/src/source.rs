use atom_vibe_build_protocol::BuildArtifactRef;
use math_atoms_hash::sha256_hex;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceViolation {
    Stub {
        path: String,
        marker: String,
    },
    Allow {
        path: String,
        line: usize,
        reason: String,
    },
}

pub(crate) fn verify_artifacts(
    root: &Path,
    artifacts: &[BuildArtifactRef],
) -> Result<PathBuf, String> {
    if !root.is_dir() {
        return Err(format!(
            "evidence root is not a directory: {}",
            root.display()
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("could not canonicalize evidence root: {error}"))?;
    for artifact in artifacts {
        artifact
            .validate()
            .map_err(|error| format!("artifact contract failed: {error}"))?;
        let path = canonical_root.join(artifact.path.replace('\\', "/"));
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("artifact {} is unavailable: {error}", artifact.path))?;
        canonical.strip_prefix(&canonical_root).map_err(|_| {
            format!(
                "artifact {} resolves outside the evidence root",
                artifact.path
            )
        })?;
        if !canonical.is_file() {
            return Err(format!("artifact {} is not a file", artifact.path));
        }
        let actual =
            sha256_hex(&fs::read(&canonical).map_err(|error| {
                format!("artifact {} could not be read: {error}", artifact.path)
            })?);
        if !actual.eq_ignore_ascii_case(&artifact.sha256_hex) {
            return Err(format!(
                "artifact {} hash does not recompute",
                artifact.path
            ));
        }
    }
    Ok(canonical_root)
}

pub(crate) fn inspect_rust_sources(
    root: &Path,
    artifacts: &[BuildArtifactRef],
    allow_couple_markers: bool,
) -> Result<Vec<String>, SourceViolation> {
    let mut markers = Vec::new();
    for artifact in artifacts {
        if !artifact.path.to_ascii_lowercase().ends_with(".rs") {
            continue;
        }
        let path = root.join(artifact.path.replace('\\', "/"));
        let source = fs::read_to_string(&path).map_err(|error| SourceViolation::Stub {
            path: artifact.path.clone(),
            marker: format!("unreadable UTF-8 source: {error}"),
        })?;
        if let Some(marker) = first_stub_marker(&source) {
            return Err(SourceViolation::Stub {
                path: artifact.path.clone(),
                marker,
            });
        }
        for (index, raw) in source.lines().enumerate() {
            let compact = raw
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            if compact.contains("#![allow(") {
                return Err(SourceViolation::Allow {
                    path: artifact.path.clone(),
                    line: index + 1,
                    reason: "crate-level warning allow is forbidden".to_string(),
                });
            }
            if let Some(position) = compact.find("#[allow(") {
                let rest = &compact[position + "#[allow(".len()..];
                let Some(end) = rest.find(")]") else {
                    return Err(SourceViolation::Allow {
                        path: artifact.path.clone(),
                        line: index + 1,
                        reason: "warning allow attribute is malformed".to_string(),
                    });
                };
                let names = rest[..end].split(',').collect::<Vec<_>>();
                if names.is_empty()
                    || names
                        .iter()
                        .any(|name| !matches!(*name, "dead_code" | "unused"))
                {
                    return Err(SourceViolation::Allow {
                        path: artifact.path.clone(),
                        line: index + 1,
                        reason: "only scoped dead_code or unused allows are eligible".to_string(),
                    });
                }
                let Some(raw_marker) = raw.split("// COUPLE:").nth(1).map(str::trim) else {
                    return Err(SourceViolation::Allow {
                        path: artifact.path.clone(),
                        line: index + 1,
                        reason: "eligible allow is missing a COUPLE consumer marker".to_string(),
                    });
                };
                if raw_marker.is_empty() {
                    return Err(SourceViolation::Allow {
                        path: artifact.path.clone(),
                        line: index + 1,
                        reason: "COUPLE marker has no consumer".to_string(),
                    });
                }
                markers.push(format!("{}:{}:{}", artifact.path, index + 1, raw_marker));
            }
            if raw.contains("COUPLE:") && !compact.contains("#[allow(") {
                return Err(SourceViolation::Allow {
                    path: artifact.path.clone(),
                    line: index + 1,
                    reason: "COUPLE marker is not attached to a scoped allow".to_string(),
                });
            }
        }
    }
    markers.sort();
    markers.dedup();
    if !allow_couple_markers && !markers.is_empty() {
        return Err(SourceViolation::Allow {
            path: markers[0].clone(),
            line: 0,
            reason: "COUPLE marker survived beyond crate build".to_string(),
        });
    }
    Ok(markers)
}

fn first_stub_marker(source: &str) -> Option<String> {
    let compact = source
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    for marker in ["todo!(", "unimplemented!("] {
        if compact.contains(marker) {
            return Some(marker.to_string());
        }
    }
    if compact.contains("panic!(\"todo")
        || compact.contains("panic!(r#\"todo")
        || compact.contains("panic!(\"placeholder")
    {
        return Some("placeholder panic".to_string());
    }
    for marker in ["placeholder", "stub"] {
        if contains_word(source, marker) {
            return Some(marker.to_string());
        }
    }
    None
}

fn contains_word(source: &str, needle: &str) -> bool {
    let lowered = source.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(offset) = lowered[cursor..].find(needle) {
        let start = cursor + offset;
        let end = start + needle.len();
        let before = lowered[..start].chars().next_back();
        let after = lowered[end..].chars().next();
        let boundary =
            |value: Option<char>| value.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        if boundary(before) && boundary(after) {
            return true;
        }
        cursor = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_words_require_boundaries_but_macros_are_deterministic() {
        assert!(first_stub_marker("fn run() { todo!() }").is_some());
        assert!(first_stub_marker("// placeholder").is_some());
        assert!(first_stub_marker("struct StubConfig;").is_none());
        assert!(first_stub_marker("let placeholder_count = 1;").is_none());
    }
}
