//! Pure record-builders for the native Atom Vibe Coder shell: turn a `RuntimeState` (plus
//! a little native context) into durable `LearningRecord` / `ProofRecord` values.
//!
//! Extracted from `math-atoms-native/src/model.rs` so that crate stays under its
//! Painted-Fence line cap. No UI, no Win32, no I/O — just deterministic construction, so
//! the native `current_*_record` methods become thin wrappers over these functions.

use std::collections::HashMap;

use math_atoms_core::{
    effective_records, LearningOutcome, LearningRecord, LearningRecordInput, LearningStore,
    LearningSummary, MathAtomsRuntime, ProofRecord, ProviderConfig, RuntimeState, RuntimeStatus,
    DEFAULT_GRAPH_MEMORY_LIMIT,
};

/// Build the learning record for the current proof/provider run, or `None` when the run
/// is neither proven nor blocked (nothing durable to record yet).
pub fn learning_record_from_state(
    state: &RuntimeState,
    last_intent: &str,
    learning_attempts: &HashMap<(String, String), u32>,
    prior_records: &[LearningRecord],
    last_provider_output: &str,
) -> Option<LearningRecord> {
    let outcome = match state.status {
        RuntimeStatus::Proven => LearningOutcome::Succeeded,
        RuntimeStatus::Blocked => LearningOutcome::Failed,
        _ => return None,
    };
    let provider_gate = state.last_provider_call.is_some()
        || state
            .blockers
            .iter()
            .any(|blocker| blocker.to_ascii_lowercase().contains("provider"));
    let gate = if provider_gate {
        "native-provider-execution"
    } else {
        "native-proof-route"
    };
    let attempt = learning_attempts
        .get(&(last_intent.to_string(), gate.to_string()))
        .copied()
        .unwrap_or(0)
        + 1;
    let correction = if outcome == LearningOutcome::Succeeded {
        prior_records
            .iter()
            .rev()
            .find(|record| {
                record.intent == last_intent
                    && record.gate == gate
                    && record.outcome == LearningOutcome::Failed
            })
            .map(|record| record.failure.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let failure = if outcome == LearningOutcome::Failed {
        let blockers = state.blockers.join("; ");
        if blockers.is_empty() {
            last_provider_output.to_string()
        } else {
            blockers
        }
    } else {
        String::new()
    };
    Some(LearningRecord::new(LearningRecordInput {
        source: "native-app".to_string(),
        intent: last_intent.to_string(),
        recipe_id: state.selected_recipe.clone(),
        atom_stack: state.selected_atoms.clone(),
        gate: gate.to_string(),
        attempt,
        outcome,
        failure,
        correction,
        artifact_path: state.last_provider_output_artifact.clone(),
        artifact_hash: state.last_provider_output_hash.clone(),
        provider_model: state
            .last_provider_call
            .as_ref()
            .map(|call| call.model.clone())
            .unwrap_or_default(),
        work_plan_id: state.last_work_plan_id.clone(),
        work_plan_manifest: state.last_work_plan_manifest.clone(),
        work_packet_count: state.last_work_packet_count,
        candidate_verification: state.last_candidate_verification.clone(),
        harness_attestation_path: String::new(),
        harness_attestation_hash: String::new(),
        route_len: state.last_route.len(),
    }))
}

/// Build the learning record for a fast-build compile FAILURE — the reusable
/// "correct this failure" lesson written back to the Wiki Graph. `fallback_model` is used
/// only when the run has no recorded provider call of its own.
pub fn fast_build_failure_record(
    state: &RuntimeState,
    last_intent: &str,
    fallback_model: &str,
    attempt: u32,
    compile_errors: &str,
    artifact_path: &str,
) -> LearningRecord {
    let provider_model = state
        .last_provider_call
        .as_ref()
        .map(|call| call.model.clone())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| fallback_model.to_string());
    LearningRecord::new(LearningRecordInput {
        source: "native-app".to_string(),
        intent: last_intent.to_string(),
        recipe_id: state.selected_recipe.clone(),
        atom_stack: state.selected_atoms.clone(),
        gate: "native-fast-build".to_string(),
        attempt,
        outcome: LearningOutcome::Failed,
        failure: compile_errors.to_string(),
        correction: String::new(),
        artifact_path: artifact_path.to_string(),
        artifact_hash: String::new(),
        provider_model,
        work_plan_id: String::new(),
        work_plan_manifest: String::new(),
        work_packet_count: 0,
        candidate_verification: None,
        harness_attestation_path: String::new(),
        harness_attestation_hash: String::new(),
        route_len: state.last_route.len(),
    })
}

/// Build the proof record for the current run. `provider_state` is the native
/// `provider_title_state()` string; provider audit fields are only filled when it is
/// `"provider:ran"`.
pub fn proof_record_from_state(state: &RuntimeState, provider_state: &str) -> ProofRecord {
    let provider = state.last_provider_call.as_ref();
    let ran = provider_state == "provider:ran";
    ProofRecord {
        recipe_id: state.selected_recipe.clone(),
        status: state.status.as_str().to_string(),
        atoms: state.selected_atoms.clone(),
        evidence_count: state.evidence.len(),
        blockers: state.blockers.clone(),
        provider_state: provider_state.to_string(),
        provider_model: provider.map(|call| call.model.clone()).unwrap_or_default(),
        provider_endpoint: provider
            .map(|call| call.endpoint.clone())
            .unwrap_or_default(),
        provider_output_artifact: if ran {
            state.last_provider_output_artifact.clone()
        } else {
            String::new()
        },
        provider_output_hash: if ran {
            state.last_provider_output_hash.clone()
        } else {
            String::new()
        },
        provider_output_len: if ran {
            state.last_provider_output_len
        } else {
            0
        },
        work_plan_id: if ran {
            state.last_work_plan_id.clone()
        } else {
            String::new()
        },
        work_plan_manifest: if ran {
            state.last_work_plan_manifest.clone()
        } else {
            String::new()
        },
        work_packet_count: if ran { state.last_work_packet_count } else { 0 },
        candidate_verification: if ran {
            state.last_candidate_verification.clone()
        } else {
            None
        },
        route_len: state.last_route.len(),
    }
}

/// Convert the runtime's `math_atoms_core::ProviderConfig` (the UI-applied provider) to
/// `avc_core::ProviderConfig` (the fast-build path's provider type). Closes the C2
/// schism: the Run button now uses the provider the operator selected, not process env.
pub fn provider_config_to_avc(cfg: &ProviderConfig) -> avc_core::ProviderConfig {
    let converted = avc_core::ProviderConfig::from_values_full(avc_core::ProviderConfigInput {
        kind_raw: cfg.kind.as_str(),
        format_raw: cfg.wire_format.as_str(),
        model: &cfg.model,
        endpoint: &cfg.endpoint,
        api_key_env: &cfg.api_key_env,
        auth_header: &cfg.auth_header,
        auth_scheme: &cfg.auth_scheme,
        body_template: &cfg.body_template,
        response_key: &cfg.response_key,
    });
    // from_values_full recomputes api_key_present from process env; if the runtime already
    // has the key marked present, honor that so a re-applied config is not down-graded.
    if cfg.api_key_present && !converted.api_key_present {
        avc_core::ProviderConfig {
            api_key_present: true,
            ..converted
        }
    } else {
        converted
    }
}

/// Persist a fast-build FAILURE as a durable learning lesson. Extracted from
/// `NativeApp::record_fast_build_learning` so the native crate stays under its line cap.
/// Only real rustc failures with error text are recorded; a fast-path success can't meet
/// the provider-grade proof gates, so the ledger never fabricates one.
#[allow(clippy::too_many_arguments)]
pub fn record_fast_build_learning(
    runtime: &mut MathAtomsRuntime,
    last_intent: &str,
    fallback_model: &str,
    learning_store: Option<&LearningStore>,
    learning_records: &mut Vec<LearningRecord>,
    learning_summary: &mut LearningSummary,
    learning_attempts: &mut HashMap<(String, String), u32>,
    build: &avc_core::FastBuild,
) {
    if !build.verified || build.compiled || build.compile_errors.is_empty() {
        return;
    }
    let gate = "native-fast-build".to_string();
    let attempt = learning_attempts
        .get(&(last_intent.to_string(), gate.clone()))
        .copied()
        .unwrap_or(0)
        + 1;
    let record = fast_build_failure_record(
        runtime.state(),
        last_intent,
        fallback_model,
        attempt,
        &build.compile_errors,
        &build.artifact.source_path,
    );
    if let Some(store) = learning_store {
        if store.append(&record).is_err() {
            return;
        }
    }
    runtime.learn_learning_record(&record);
    learning_attempts.insert((last_intent.to_string(), gate), attempt);
    learning_records.push(record);
    *learning_records = effective_records(learning_records, DEFAULT_GRAPH_MEMORY_LIMIT);
    learning_summary.total += 1;
    learning_summary.failed += 1;
}

/// Convert a bridge `append_...` result into a UI-facing warning string when the
/// scratchpad backend is dead. Returns `None` when the write actually landed so the
/// caller can no-op. H6: no more silent `let _`.
pub fn warn_if_memory_lost(result: Result<bool, String>, label: &str) -> Option<String> {
    match result {
        Ok(true) => None,
        Ok(false) => Some(format!(
            "\nWARNING: scratchpad memory backend unavailable; {label} not persisted."
        )),
        Err(error) => Some(format!(
            "\nWARNING: scratchpad append failed for {label}: {error}"
        )),
    }
}
