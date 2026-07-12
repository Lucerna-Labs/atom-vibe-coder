use atom_vibe_build_protocol::BuildStep;

pub fn step_output_contract(step: BuildStep) -> &'static str {
    match step {
        BuildStep::Intake => {
            r#"Return one JSON object with schema_version, build_id, step="intake", summary, and payload. Payload must contain purpose, user_behaviors, ui_decision, persistence_decision, external_boundaries, execution_siting, out_of_scope, and definition_of_done. Do not include architecture, crates, code, or claimed test results."#
        }
        BuildStep::Blueprint => {
            r#"Return one JSON object with schema_version, build_id, step="blueprint", summary, and payload. Payload must contain version, crates(name,responsibility), message_contracts(id,message_type,producer,consumers,failure_semantics), dependency_edges(dependency,consumer), topological_order, coupling_order, and independent_review_request. Do not implement files or claim review approval."#
        }
        BuildStep::CrateBuild => {
            r#"Return one JSON object with schema_version, build_id, step="crate_build", summary, and payload. Payload must identify exactly one next frozen crate and include complete files(path,content), required warning-denied commands, focused real unit tests, and any exact COUPLE markers. Do not wire another crate or claim commands ran."#
        }
        BuildStep::CrateCouple => {
            r#"Return one JSON object with schema_version, build_id, step="crate_couple", summary, and payload. Payload must identify exactly one next frozen message contract, include complete changed files, the producer emission, consumer handling, real round-trip command, removed COUPLE markers, and an empty deferred_reason unless a later frozen wiring is genuinely required. Do not claim execution results."#
        }
        BuildStep::BuildTest => {
            r#"Return one JSON object with schema_version, build_id, step="build_test", summary, and payload. Payload must contain warning-denied check/test/Clippy commands, one real workflow per definition-of-done requirement, persistence and Spiderweb round-trip probes, and an independent_review_request. Do not return smoke-only checks or claimed pass results."#
        }
        BuildStep::LaunchProof => {
            r#"Return one JSON object with schema_version, build_id, step="launch_proof", summary, and payload. Payload must contain the operator launch command, usable-screen observation method, screenshot target, real interaction steps, expected L0-L3 event/result/render-state evidence, definition-of-done checks, and process-liveness check. Do not claim the app launched."#
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_contract_rejects_claimed_execution_as_evidence() {
        for step in BuildStep::ALL {
            let contract = step_output_contract(step);
            assert!(contract.contains("JSON object"));
            assert!(contract.contains(step.as_str()));
            assert!(contract.contains("Do not"));
        }
    }
}
