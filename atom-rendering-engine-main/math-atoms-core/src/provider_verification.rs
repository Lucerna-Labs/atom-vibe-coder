use crate::provider::{
    provider_body, provider_output_hash, CandidateVerificationReport, PreparedProviderCall,
    ProviderError,
};
use math_atoms_repair::{repair_prompt, repair_target_indices};
use math_atoms_verification::{
    candidate_output, verify_candidate_evidence, CandidateFile, CandidateVerifier,
    VerificationAttempt, VerificationError, VerificationPolicy,
};
use math_atoms_work::{validate_secure_file_artifact, CompletedPacket, GeneratedFile, WorkPlan};
use std::path::Path;
use std::time::{Duration, Instant};

const REPAIR_RESPONSE_ATTEMPTS: u32 = 3;

pub(crate) struct ClosedCandidate {
    pub text: String,
    pub report: CandidateVerificationReport,
}

pub(crate) fn close_candidate_loop(
    call: &PreparedProviderCall,
    plan: &WorkPlan,
    completed: &[CompletedPacket],
    work_root: &Path,
    deadline: Instant,
) -> Result<ClosedCandidate, ProviderError> {
    let generated = plan
        .generated_files(completed)
        .map_err(provider_work_error)?;
    let mut files = candidate_files(&generated)?;

    for ordinal in 1..=call.verification_attempt_limit {
        let timeout = verification_timeout(call, deadline)?;
        let verifier = CandidateVerifier::new(
            work_root,
            VerificationPolicy::strict(timeout).map_err(verification_error)?,
        );
        let attempt = verifier
            .verify_attempt(&plan.id, ordinal, &files)
            .map_err(verification_error)?;
        if attempt.passed {
            let success = verifier.finalize(&attempt).map_err(verification_error)?;
            let verified = verify_candidate_evidence(
                &success.manifest_path,
                &success.manifest_hash,
                &plan.id,
                &success.bundle_hash,
            )
            .map_err(verification_error)?;
            let text = candidate_output(&files).map_err(verification_error)?;
            if provider_output_hash(&text) != success.bundle_hash {
                return Err(ProviderError::WorkPacketFailed(format!(
                    "plan {} final provider output is not the candidate that passed verification",
                    plan.id
                )));
            }
            return Ok(ClosedCandidate {
                text,
                report: CandidateVerificationReport {
                    manifest_path: success.manifest_path.to_string_lossy().to_string(),
                    manifest_hash: success.manifest_hash,
                    bundle_hash: success.bundle_hash,
                    attempts: verified.attempts,
                    repairs: verified.repairs,
                },
            });
        }
        if ordinal == call.verification_attempt_limit {
            return Err(ProviderError::WorkPacketFailed(format!(
                "plan {} exhausted {} real verification attempts; final failure: {}",
                plan.id, call.verification_attempt_limit, attempt.failure
            )));
        }

        files = if let Some(repair) = verifier
            .load_repair(&plan.id, ordinal)
            .map_err(verification_error)?
        {
            if repair.source_bundle_hash != attempt.bundle_hash {
                return Err(ProviderError::WorkPacketFailed(format!(
                    "plan {} stored repair is not bound to verification attempt {ordinal}",
                    plan.id
                )));
            }
            repair.files
        } else {
            let repaired = repair_candidate(call, plan, &attempt, &files, deadline)?;
            verifier
                .store_repair(&attempt, &call.model, &repaired)
                .map_err(verification_error)?
                .files
        };
    }

    Err(ProviderError::WorkPacketFailed(format!(
        "plan {} verification loop terminated without a result",
        plan.id
    )))
}

fn candidate_files(generated: &[GeneratedFile]) -> Result<Vec<CandidateFile>, ProviderError> {
    generated
        .iter()
        .map(|file| CandidateFile::new(&file.path, &file.content).map_err(verification_error))
        .collect()
}

fn repair_candidate(
    call: &PreparedProviderCall,
    plan: &WorkPlan,
    failed: &VerificationAttempt,
    files: &[CandidateFile],
    deadline: Instant,
) -> Result<Vec<CandidateFile>, ProviderError> {
    let targets = repair_target_indices(files, &failed.failure);
    let mut repaired = files.to_vec();
    for index in targets {
        let content = request_file_repair(call, plan, failed, &repaired, index, deadline)?;
        repaired[index] =
            CandidateFile::new(&repaired[index].path, content).map_err(verification_error)?;
    }
    Ok(repaired)
}

fn request_file_repair(
    call: &PreparedProviderCall,
    plan: &WorkPlan,
    failed: &VerificationAttempt,
    files: &[CandidateFile],
    target: usize,
    deadline: Instant,
) -> Result<String, ProviderError> {
    let current = &files[target];
    let mut response_problem = String::new();
    for response_attempt in 1..=REPAIR_RESPONSE_ATTEMPTS {
        let prompt = repair_prompt(plan, failed, files, target, &response_problem);
        let body = provider_body(
            call.wire_format,
            &call.model,
            &prompt,
            &call.body_template,
            call.thinking_level,
        );
        let timeout = request_timeout(call, deadline)?;
        let raw = call
            .execute_body_with_curl_timeout(&body, timeout)
            .map_err(|error| {
                ProviderError::WorkPacketFailed(format!(
                    "plan {} repair call for {} failed: {error:?}",
                    plan.id, current.path
                ))
            })?;
        match validate_secure_file_artifact(&current.path, &raw) {
            Ok(content) if content != current.content => return Ok(content),
            Ok(_) => {
                response_problem = "the response repeated the failed file byte-for-byte".to_string()
            }
            Err(error) => response_problem = error.to_string(),
        }
        if response_attempt == REPAIR_RESPONSE_ATTEMPTS {
            return Err(ProviderError::WorkPacketFailed(format!(
                "plan {} repair for {} failed its response contract after {} attempts: {}",
                plan.id, current.path, REPAIR_RESPONSE_ATTEMPTS, response_problem
            )));
        }
    }
    unreachable!("bounded repair response loop always returns")
}

fn verification_timeout(
    call: &PreparedProviderCall,
    deadline: Instant,
) -> Result<u64, ProviderError> {
    let remaining = remaining(deadline)?;
    let per_command = remaining.as_secs() / 3;
    if per_command < 10 {
        return Err(ProviderError::WorkPacketFailed(
            "provider plan has less than 30 seconds for strict candidate verification".to_string(),
        ));
    }
    Ok(call.verification_timeout_seconds.min(per_command))
}

fn request_timeout(call: &PreparedProviderCall, deadline: Instant) -> Result<u64, ProviderError> {
    Ok(call
        .request_timeout_seconds
        .min(remaining(deadline)?.as_secs().max(1)))
}

fn remaining(deadline: Instant) -> Result<Duration, ProviderError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ProviderError::WorkPacketFailed(
            "provider plan exhausted its total execution budget".to_string(),
        ));
    }
    Ok(remaining)
}

fn verification_error(error: VerificationError) -> ProviderError {
    ProviderError::WorkPacketFailed(error.to_string())
}

fn provider_work_error(error: math_atoms_work::WorkError) -> ProviderError {
    ProviderError::WorkPacketFailed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;
    use math_atoms_json::parse as parse_json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    const BROKEN_SOURCE: &str = "pub fn answer() -> i32 { missing_name }\n";
    const FIXED_SOURCE: &str = "pub fn answer() -> i32 { 42 }\n#[cfg(test)] mod tests { use super::*; #[test] fn answer_is_42() { assert_eq!(answer(), 42); } }\n";

    struct EnvGuard {
        values: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&str, &str)]) -> Self {
            let previous = values
                .iter()
                .map(|(name, value)| {
                    let old = std::env::var(name).ok();
                    std::env::set_var(name, value);
                    ((*name).to_string(), old)
                })
                .collect();
            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in self.values.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    #[test]
    fn loopback_provider_repairs_a_real_compiler_failure_before_release() {
        let root = std::env::temp_dir().join(format!(
            "math-atoms-provider-loop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let saw_failure = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server_requests = requests.clone();
        let server_saw_failure = saw_failure.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(120);
            while !server_stop.load(Ordering::SeqCst) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        server_requests.fetch_add(1, Ordering::SeqCst);
                        let body = read_request_body(&mut stream);
                        let response = scripted_response(&body, &server_saw_failure);
                        let envelope = format!(
                            "{{\"choices\":[{{\"message\":{{\"content\":\"{}\",\"reasoning_content\":\"loopback reasoning\"}}}}],\"usage\":{{\"completion_tokens_details\":{{\"reasoning_tokens\":8}}}}}}",
                            escape_json(&response)
                        );
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            envelope.len(),
                            envelope
                        )
                        .unwrap();
                        stream.flush().unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback provider failed: {error}"),
                }
            }
        });

        let key_env = format!("MATH_ATOMS_LOOP_KEY_{}", std::process::id());
        let root_text = root.to_string_lossy().to_string();
        let _env = EnvGuard::set(&[
            (&key_env, "loopback-secret"),
            ("MATH_ATOMS_WORK_DIR", &root_text),
        ]);
        let endpoint = format!("http://{address}/v1/chat/completions");
        let config = ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", "custom"),
            ("MATH_ATOMS_PROVIDER_FORMAT", "chat"),
            ("MATH_ATOMS_PROVIDER_MODEL", "loopback-model"),
            ("MATH_ATOMS_PROVIDER_URL", &endpoint),
            ("MATH_ATOMS_PROVIDER_KEY_ENV", &key_env),
            (key_env.as_str(), "loopback-secret"),
            ("MATH_ATOMS_VERIFICATION_MAX_ATTEMPTS", "4"),
            ("MATH_ATOMS_VERIFICATION_TIMEOUT_SECONDS", "60"),
        ]);
        let call = config
            .prepare_call(
                "Build a dependency-free answer library",
                "provider-model-loop",
                &[],
            )
            .unwrap();
        let result = call.execute_with_curl_report();
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
        let report = result.unwrap();
        let verification = report.candidate_verification.as_ref().unwrap();

        assert_eq!(report.text, FIXED_SOURCE);
        assert_eq!(report.packet_ids.len(), 19);
        assert_eq!(report.executed_packets, 19);
        assert_eq!(report.resumed_packets, 0);
        assert_eq!(verification.attempts, 2);
        assert_eq!(verification.repairs, 1);
        assert!(saw_failure.load(Ordering::SeqCst));
        assert_eq!(requests.load(Ordering::SeqCst), 20);
        verify_candidate_evidence(
            &verification.manifest_path,
            &verification.manifest_hash,
            &report.work_plan_id,
            &verification.bundle_hash,
        )
        .unwrap();
        let first_attempt = std::fs::read_to_string(
            root.join(&report.work_plan_id)
                .join("candidate-verification")
                .join("attempt-001")
                .join("attempt.json"),
        )
        .unwrap();
        assert!(first_attempt.contains("missing_name"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn scripted_response(body: &str, saw_failure: &AtomicBool) -> String {
        let value = parse_json(body).unwrap();
        let instructions = value
            .get("messages")
            .and_then(math_atoms_json::JsonValue::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("content"))
            .and_then(math_atoms_json::JsonValue::as_str)
            .unwrap();
        let data = value
            .get("messages")
            .and_then(math_atoms_json::JsonValue::as_array)
            .and_then(|messages| messages.get(1))
            .and_then(|message| message.get("content"))
            .and_then(math_atoms_json::JsonValue::as_str)
            .unwrap();
        if instructions.contains("failed-gate repair controller") {
            assert!(data.contains("missing_name"));
            assert!(data.contains("cargo-check failed"));
            saw_failure.store(true, Ordering::SeqCst);
            return format!("```rust\n{FIXED_SOURCE}```");
        }
        let packet_id = instructions
            .lines()
            .find_map(|line| line.strip_prefix("Packet id: "))
            .unwrap();
        if instructions.contains("Stage: file-manifest") {
            return format!(
                "{{\"packet_id\":\"{packet_id}\",\"status\":\"complete\",\"files\":[{{\"path\":\"src/lib.rs\",\"purpose\":\"answer library\",\"acceptance\":[\"returns 42 and passes strict Cargo gates\"]}}],\"checks\":[\"one dependency-free file owns the behavior\"],\"risks\":[]}}"
            );
        }
        if instructions.contains("Required output contract:\nReturn only the complete contents") {
            let source = if instructions.contains("Stage: final-correction") {
                BROKEN_SOURCE
            } else {
                "pub fn answer() -> i32 { 42 }\n"
            };
            return format!("```rust\n{source}```");
        }
        format!(
            "{{\"packet_id\":\"{packet_id}\",\"status\":\"complete\",\"result\":\"packet completed\",\"checks\":[\"contract checked\"],\"risks\":[]}}"
        )
    }

    fn read_request_body(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap();
        while bytes.len() - header_end < content_length {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        String::from_utf8(bytes[header_end..header_end + content_length].to_vec()).unwrap()
    }

    fn escape_json(value: &str) -> String {
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
}
