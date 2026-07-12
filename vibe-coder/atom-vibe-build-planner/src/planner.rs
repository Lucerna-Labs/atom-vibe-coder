use crate::model::{
    BuildLedger, BuildPlannerDecision, BuildPlannerError, BuildRunStatus, DeferredDebt,
    RetryRecord, StepOutput, WiringLedgerRecord, BUILD_LEDGER_SCHEMA_VERSION,
};
use atom_vibe_build_gates::{evaluate_gate, GateInput};
use atom_vibe_build_protocol::{
    unix_time_ms, BuildErrorClass, BuildGateEvidence, BuildPlannerEvent, BuildStep, GateOutcome,
};
use math_atoms_hash::sha256_hex;
use std::sync::atomic::{AtomicU64, Ordering};

static BUILD_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct BuildPlanner {
    ledger: BuildLedger,
}

impl BuildPlanner {
    pub fn new(
        project_id: impl Into<String>,
        autonomous_correction: bool,
        retry_limit: u8,
    ) -> Result<Self, BuildPlannerError> {
        let project_id = project_id.into();
        let now = unix_time_ms();
        let sequence = BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let identity = format!("{project_id}\0{now}\0{}\0{sequence}", std::process::id());
        let build_id = format!("build-{}", &sha256_hex(identity.as_bytes())[..24]);
        let ledger = BuildLedger {
            schema_version: BUILD_LEDGER_SCHEMA_VERSION,
            revision: 1,
            build_id,
            project_id,
            status: BuildRunStatus::Active,
            current_step: BuildStep::Intake,
            autonomous_correction,
            retry_limit,
            requirements: None,
            blueprint_versions: Vec::new(),
            step_outputs: Vec::new(),
            crate_statuses: Vec::new(),
            wiring_statuses: Vec::new(),
            deferred_debt: Vec::new(),
            couple_markers: Vec::new(),
            retry_ledger: Vec::new(),
            launch_proof: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        ledger.validate()?;
        Ok(Self { ledger })
    }

    pub fn from_ledger(ledger: BuildLedger) -> Result<Self, BuildPlannerError> {
        ledger.validate()?;
        Ok(Self { ledger })
    }

    pub fn ledger(&self) -> &BuildLedger {
        &self.ledger
    }

    pub fn start_event(&self) -> BuildPlannerEvent {
        BuildPlannerEvent::BuildStarted {
            build_id: self.ledger.build_id.clone(),
            project_id: self.ledger.project_id.clone(),
            released_skill: self.ledger.current_skill().to_string(),
        }
    }

    pub fn evaluate_and_handle(
        &mut self,
        input: &GateInput,
    ) -> Result<BuildPlannerDecision, BuildPlannerError> {
        if self.ledger.status != BuildRunStatus::Active {
            return Err(BuildPlannerError::BuildNotActive);
        }
        if input.step != self.ledger.current_step {
            return Err(BuildPlannerError::WrongStep {
                expected: self.ledger.current_step,
                actual: input.step,
            });
        }
        let outcome = evaluate_gate(input);
        let decision = match outcome {
            GateOutcome::Pass { evidence } => self.handle_pass(input, evidence)?,
            GateOutcome::Fail {
                error_class,
                evidence,
            } => self.handle_fail(input.step, error_class, evidence),
            GateOutcome::Deferred { reason, evidence } => {
                self.handle_deferred(input.step, reason, evidence)?
            }
        };
        self.ledger.revision = self.ledger.revision.saturating_add(1);
        self.ledger.updated_at_unix_ms = unix_time_ms();
        self.ledger.validate()?;
        Ok(decision)
    }

    fn handle_pass(
        &mut self,
        input: &GateInput,
        evidence: BuildGateEvidence,
    ) -> Result<BuildPlannerDecision, BuildPlannerError> {
        let step = input.step;
        if matches!(step, BuildStep::BuildTest | BuildStep::LaunchProof)
            && !input.deferred_debt.is_empty()
        {
            return Err(BuildPlannerError::DeferredDebtRemaining);
        }
        if matches!(
            step,
            BuildStep::CrateCouple | BuildStep::BuildTest | BuildStep::LaunchProof
        ) && !input.couple_markers.is_empty()
        {
            return Err(BuildPlannerError::CoupleMarkersRemaining);
        }
        self.record_step_state(input);
        self.ledger.step_outputs.push(StepOutput {
            step,
            summary: evidence.summary,
            details: evidence.details,
            artifacts: evidence.artifacts,
            recorded_at_unix_ms: evidence.checked_at_unix_ms,
        });
        let next_step = step.next();
        if let Some(next) = next_step {
            self.ledger.current_step = next;
        } else {
            self.ledger.status = BuildRunStatus::Complete;
        }
        Ok(BuildPlannerDecision::Advance(
            BuildPlannerEvent::StepAdvanced {
                build_id: self.ledger.build_id.clone(),
                project_id: self.ledger.project_id.clone(),
                completed_step: step,
                next_step,
                released_skill: next_step.map(|next| next.skill_id().to_string()),
            },
        ))
    }

    fn handle_fail(
        &mut self,
        step: BuildStep,
        error_class: BuildErrorClass,
        evidence: BuildGateEvidence,
    ) -> BuildPlannerDecision {
        let attempt = self.ledger.retry_attempts(step).saturating_add(1);
        let retry_eligible = self.ledger.autonomous_correction
            && error_class.is_bounded_candidate()
            && attempt <= usize::from(self.ledger.retry_limit);
        let (decision_label, event) = if retry_eligible {
            let event = BuildPlannerEvent::AutonomousRetryRequested {
                build_id: self.ledger.build_id.clone(),
                project_id: self.ledger.project_id.clone(),
                step,
                error_class,
                attempt: attempt as u8,
                attempt_limit: self.ledger.retry_limit,
                summary: evidence.summary.clone(),
            };
            ("retry_requested", event)
        } else {
            self.ledger.status = BuildRunStatus::Halted;
            let exhausted = self.ledger.autonomous_correction
                && error_class.is_bounded_candidate()
                && attempt > usize::from(self.ledger.retry_limit);
            let final_class = if exhausted {
                BuildErrorClass::RetryExhausted
            } else {
                error_class
            };
            let summary = if exhausted {
                format!(
                    "{}; {} autonomous corrections were exhausted",
                    evidence.summary, self.ledger.retry_limit
                )
            } else {
                evidence.summary.clone()
            };
            let event = BuildPlannerEvent::HardHalted {
                build_id: self.ledger.build_id.clone(),
                project_id: self.ledger.project_id.clone(),
                step,
                error_class: final_class,
                summary,
            };
            ("hard_halt", event)
        };
        self.ledger.retry_ledger.push(RetryRecord {
            step,
            error_class,
            attempt: attempt.min(usize::from(u8::MAX)) as u8,
            summary: evidence.summary,
            artifacts: evidence.artifacts,
            decision: decision_label.to_string(),
            recorded_at_unix_ms: evidence.checked_at_unix_ms,
        });
        if retry_eligible {
            BuildPlannerDecision::AutonomousRetry(event)
        } else {
            BuildPlannerDecision::HardHalt(event)
        }
    }

    fn handle_deferred(
        &mut self,
        step: BuildStep,
        reason: String,
        evidence: BuildGateEvidence,
    ) -> Result<BuildPlannerDecision, BuildPlannerError> {
        if step != BuildStep::CrateCouple {
            return Err(BuildPlannerError::DeferredOutsideCouple);
        }
        self.ledger.deferred_debt.push(DeferredDebt {
            step,
            reason: reason.clone(),
            recorded_at_unix_ms: evidence.checked_at_unix_ms,
        });
        Ok(BuildPlannerDecision::Deferred(
            BuildPlannerEvent::DeferredDebtRecorded {
                build_id: self.ledger.build_id.clone(),
                project_id: self.ledger.project_id.clone(),
                step,
                reason,
            },
        ))
    }

    fn record_step_state(&mut self, input: &GateInput) {
        match input.step {
            BuildStep::Intake => {
                self.ledger.requirements = input.requirements.clone();
            }
            BuildStep::Blueprint => {
                if let Some(blueprint) = &input.blueprint {
                    self.ledger.blueprint_versions.push(blueprint.clone());
                }
            }
            BuildStep::CrateBuild => {
                self.ledger.crate_statuses = input
                    .crates
                    .iter()
                    .map(|item| item.crate_name.clone())
                    .collect();
                self.ledger.couple_markers = input.couple_markers.clone();
            }
            BuildStep::CrateCouple => {
                self.ledger.wiring_statuses = input
                    .wirings
                    .iter()
                    .map(|item| WiringLedgerRecord {
                        contract_id: item.contract_id.clone(),
                        status: item.status,
                        evidence: item.evidence.clone(),
                    })
                    .collect();
                self.ledger.deferred_debt = input
                    .deferred_debt
                    .iter()
                    .map(|reason| DeferredDebt {
                        step: BuildStep::CrateCouple,
                        reason: reason.clone(),
                        recorded_at_unix_ms: unix_time_ms(),
                    })
                    .collect();
                self.ledger.couple_markers.clear();
            }
            BuildStep::BuildTest => {
                self.ledger.deferred_debt.clear();
                self.ledger.couple_markers.clear();
            }
            BuildStep::LaunchProof => {
                self.ledger.launch_proof = input.launch.clone();
                self.ledger.deferred_debt.clear();
                self.ledger.couple_markers.clear();
            }
        }
    }
}
