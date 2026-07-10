use crate::model::{
    PacketContract, ValidatedPacketOutput, WorkError, WorkFile, WorkPacket, MAX_PACKET_OUTPUT_BYTES,
};
use math_atoms_json::{parse as parse_json, JsonValue};
use math_atoms_secrets::{contains_credential_material, redact_sensitive_text};
use std::collections::HashSet;

pub fn validate_packet_output(
    packet: &WorkPacket,
    output: &str,
) -> Result<ValidatedPacketOutput, WorkError> {
    if output.len() > packet.max_output_bytes || output.len() > MAX_PACKET_OUTPUT_BYTES {
        return Err(WorkError::OutputTooLarge {
            packet_id: packet.id.clone(),
            limit: packet.max_output_bytes.min(MAX_PACKET_OUTPUT_BYTES),
        });
    }
    if output.trim().is_empty() {
        return Err(WorkError::InvalidOutput(format!(
            "packet {} returned empty output",
            packet.id
        )));
    }
    match packet.contract {
        PacketContract::Envelope => validate_envelope(packet, output),
        PacketContract::FileManifest => validate_manifest(packet, output),
        PacketContract::FileArtifact => validate_file_artifact(packet, output),
    }
}

pub fn validate_secure_packet_output(
    packet: &WorkPacket,
    output: &str,
) -> Result<ValidatedPacketOutput, WorkError> {
    match packet.contract {
        PacketContract::FileArtifact => {
            if contains_credential_material(output) {
                return Err(WorkError::InvalidOutput(format!(
                    "packet {} file artifact contains credential material",
                    packet.id
                )));
            }
            validate_packet_output(packet, output)
        }
        PacketContract::Envelope | PacketContract::FileManifest => {
            validate_packet_output(packet, &redact_sensitive_text(output))
        }
    }
}

pub fn extract_json_payload(output: &str) -> Result<&str, WorkError> {
    let trimmed = output.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(WorkError::InvalidOutput(
            "JSON packet must contain one raw object without a fence or prose".to_string(),
        ));
    }
    Ok(trimmed)
}

fn validate_envelope(
    packet: &WorkPacket,
    output: &str,
) -> Result<ValidatedPacketOutput, WorkError> {
    let value = parse_json(extract_json_payload(output)?)
        .map_err(|error| WorkError::InvalidOutput(error.to_string()))?;
    require_exact_fields(
        &value,
        &["packet_id", "status", "result", "checks", "risks"],
    )?;
    require_identity(packet, &value)?;
    let result = required_string(&value, "result")?;
    if result.trim().is_empty() {
        return Err(WorkError::InvalidOutput(
            "packet result is empty".to_string(),
        ));
    }
    let checks = required_strings(&value, "checks", true)?;
    let risks = required_strings(&value, "risks", false)?;
    let _ = (checks, risks);
    Ok(ValidatedPacketOutput {
        context: output.trim().to_string(),
        files: Vec::new(),
    })
}

fn validate_manifest(
    packet: &WorkPacket,
    output: &str,
) -> Result<ValidatedPacketOutput, WorkError> {
    let value = parse_json(extract_json_payload(output)?)
        .map_err(|error| WorkError::InvalidOutput(error.to_string()))?;
    require_exact_fields(&value, &["packet_id", "status", "files", "checks", "risks"])?;
    require_identity(packet, &value)?;
    let checks = required_strings(&value, "checks", true)?;
    let risks = required_strings(&value, "risks", false)?;
    let entries = value
        .get("files")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| WorkError::InvalidManifest("files must be an array".to_string()))?;
    let mut files = Vec::new();
    for entry in entries {
        require_exact_fields(entry, &["path", "purpose", "acceptance"])?;
        files.push(WorkFile {
            path: required_string(entry, "path")?.trim().to_string(),
            purpose: required_string(entry, "purpose")?.trim().to_string(),
            acceptance: required_strings(entry, "acceptance", true)?,
        });
    }
    let _ = (checks, risks);
    Ok(ValidatedPacketOutput {
        context: output.trim().to_string(),
        files,
    })
}

fn validate_file_artifact(
    packet: &WorkPacket,
    output: &str,
) -> Result<ValidatedPacketOutput, WorkError> {
    let trimmed = output.trim();
    if !trimmed.starts_with("```") || !trimmed.ends_with("```") {
        return Err(WorkError::InvalidOutput(format!(
            "packet {} must return exactly one fenced file",
            packet.id
        )));
    }
    let first_newline = trimmed.find('\n').ok_or_else(|| {
        WorkError::InvalidOutput(format!("packet {} file fence has no content", packet.id))
    })?;
    let content_end = trimmed.len() - 3;
    if content_end <= first_newline + 1
        || trimmed[first_newline + 1..content_end].trim().is_empty()
        || trimmed[first_newline + 1..content_end].contains("```")
    {
        return Err(WorkError::InvalidOutput(format!(
            "packet {} contains an empty or multiple file fence",
            packet.id
        )));
    }
    for marker in [
        "todo!",
        "unimplemented!",
        "FIXME",
        "panic!(\"TODO\")",
        "[placeholder]",
    ] {
        if trimmed.contains(marker) {
            return Err(WorkError::InvalidOutput(format!(
                "packet {} contains forbidden placeholder marker {marker}",
                packet.id
            )));
        }
    }
    Ok(ValidatedPacketOutput {
        context: trimmed.to_string(),
        files: Vec::new(),
    })
}

fn require_identity(packet: &WorkPacket, value: &JsonValue) -> Result<(), WorkError> {
    let packet_id = required_string(value, "packet_id")?;
    if packet_id != packet.id {
        return Err(WorkError::InvalidOutput(format!(
            "packet id mismatch: expected {}, got {packet_id}",
            packet.id
        )));
    }
    if required_string(value, "status")? != "complete" {
        return Err(WorkError::InvalidOutput(format!(
            "packet {} status must be complete",
            packet.id
        )));
    }
    Ok(())
}

fn require_exact_fields(value: &JsonValue, expected: &[&str]) -> Result<(), WorkError> {
    let object = value
        .as_object()
        .ok_or_else(|| WorkError::InvalidOutput("packet output must be an object".to_string()))?;
    let actual: HashSet<&str> = object.iter().map(|(name, _)| name.as_str()).collect();
    let expected: HashSet<&str> = expected.iter().copied().collect();
    if actual != expected {
        return Err(WorkError::InvalidOutput(format!(
            "packet fields differ: expected {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

fn required_string<'a>(value: &'a JsonValue, key: &str) -> Result<&'a str, WorkError> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| WorkError::InvalidOutput(format!("{key} must be a string")))
}

fn required_strings(
    value: &JsonValue,
    key: &str,
    non_empty: bool,
) -> Result<Vec<String>, WorkError> {
    let values = value
        .get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| WorkError::InvalidOutput(format!("{key} must be an array")))?;
    if non_empty && values.is_empty() {
        return Err(WorkError::InvalidOutput(format!("{key} must not be empty")));
    }
    let mut output = Vec::new();
    for value in values {
        let item = value
            .as_str()
            .ok_or_else(|| WorkError::InvalidOutput(format!("{key} entries must be strings")))?
            .trim();
        if item.is_empty() {
            return Err(WorkError::InvalidOutput(format!(
                "{key} contains an empty entry"
            )));
        }
        output.push(item.to_string());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorkPlan, WorkStage};

    fn packet(stage: WorkStage) -> WorkPacket {
        let mut plan =
            WorkPlan::meticulous("Build a calculator", "provider-model-loop", &[], "fixture")
                .unwrap();
        if stage == WorkStage::FileImplementation {
            plan.expand_files(vec![WorkFile {
                path: "src/main.rs".into(),
                purpose: "entry".into(),
                acceptance: vec!["calculates".into()],
            }])
            .unwrap();
        }
        plan.packets
            .into_iter()
            .find(|packet| packet.stage == stage)
            .unwrap()
    }

    #[test]
    fn envelope_is_strict_and_id_bound() {
        let packet = packet(WorkStage::Intent);
        let valid = format!(
            "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"normalized\",\"checks\":[\"intent preserved\"],\"risks\":[]}}",
            packet.id
        );
        assert!(validate_packet_output(&packet, &valid).is_ok());
        assert!(validate_packet_output(&packet, &format!("{valid} trailing")).is_err());
        assert!(validate_packet_output(&packet, &valid.replace(&packet.id, "wrong")).is_err());
        assert!(validate_packet_output(
            &packet,
            &valid.replace("\"risks\":[]", "\"risks\":[],\"extra\":true")
        )
        .is_err());
    }

    #[test]
    fn manifest_parses_focused_files() {
        let packet = packet(WorkStage::FileManifest);
        let output = format!(
            "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"src/main.rs\",\"purpose\":\"entry\",\"acceptance\":[\"runs\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
            packet.id
        );
        let validated = validate_packet_output(&packet, &output).unwrap();
        assert_eq!(validated.files[0].path, "src/main.rs");
        assert!(validated.context.starts_with('{'));
    }

    #[test]
    fn artifact_requires_one_complete_fence_and_rejects_placeholders() {
        let packet = packet(WorkStage::FileImplementation);
        assert!(validate_packet_output(&packet, "```rust\nfn main() {}\n```").is_ok());
        assert!(validate_packet_output(&packet, "fn main() {}").is_err());
        assert!(validate_packet_output(&packet, "```rust\ntodo!()\n```").is_err());
        assert!(validate_packet_output(&packet, "```rust\nfn main() {}\n```\nprose").is_err());
    }

    #[test]
    fn secure_artifact_preserves_non_secret_code_and_rejects_credentials() {
        let packet = packet(WorkStage::FileImplementation);
        let code = "```rust\nfn main() { let key=42; let token_count=3; }\n```";
        assert_eq!(
            validate_secure_packet_output(&packet, code)
                .unwrap()
                .context,
            code
        );
        assert!(validate_secure_packet_output(
            &packet,
            "```rust\nconst API: &str = \"sk-abcdefghijklmnopqrstuvwxyz\";\n```"
        )
        .is_err());
    }
}
