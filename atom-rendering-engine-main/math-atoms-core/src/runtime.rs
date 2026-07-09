use crate::bus::{BusMessageKind, EnvelopeId, SpiderwebBus};
use crate::domain::{atom_by_key, mission, recipes, Recipe};
use crate::graph::{Evidence, WikiGraph};
use crate::provider::{PreparedProviderCall, ProviderConfig, ProviderError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeStatus {
    Draft,
    Proven,
    Blocked,
    DriftFlagged,
}

impl RuntimeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proven => "proven",
            Self::Blocked => "blocked",
            Self::DriftFlagged => "drift flagged",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeState {
    pub selected_recipe: String,
    pub selected_atoms: Vec<String>,
    pub status: RuntimeStatus,
    pub proof_count: u32,
    pub drift_count: u32,
    pub evidence: Vec<Evidence>,
    pub blockers: Vec<String>,
    pub last_provider_call: Option<PreparedProviderCall>,
    pub last_route: Vec<EnvelopeId>,
}

#[derive(Clone, Debug)]
pub struct ProofRun {
    pub recipe_id: String,
    pub atom_keys: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub provider_call: Option<PreparedProviderCall>,
    pub blockers: Vec<String>,
    pub proof_envelope: EnvelopeId,
    pub status: RuntimeStatus,
}

#[derive(Clone, Debug)]
pub struct MathAtomsRuntime {
    bus: SpiderwebBus,
    graph: WikiGraph,
    provider: ProviderConfig,
    state: RuntimeState,
}

impl MathAtomsRuntime {
    pub fn new(provider: ProviderConfig) -> Self {
        Self {
            bus: SpiderwebBus::new(),
            graph: WikiGraph::seeded(),
            provider,
            state: RuntimeState {
                selected_recipe: "native-atom-renderer".to_string(),
                selected_atoms: Vec::new(),
                status: RuntimeStatus::Draft,
                proof_count: 0,
                drift_count: 0,
                evidence: Vec::new(),
                blockers: Vec::new(),
                last_provider_call: None,
                last_route: Vec::new(),
            },
        }
    }

    pub fn from_process_env() -> Self {
        Self::new(ProviderConfig::from_process_env())
    }

    pub fn bus(&self) -> &SpiderwebBus {
        &self.bus
    }

    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    pub fn provider(&self) -> &ProviderConfig {
        &self.provider
    }

    pub fn run_intent(&mut self, intent: &str) -> ProofRun {
        let l0 = self.bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator",
            "math-atoms-runtime",
            intent,
        );
        let atoms = classify_intent(intent);
        let atom_body = atoms.join(",");
        let l1 = self.bus.l1_message(
            l0,
            BusMessageKind::IntentClassified,
            "classifier",
            "wiki-graph",
            &atom_body,
        );
        let evidence = self.graph.retrieve(intent, &atoms, 8);
        let evidence_ids: Vec<String> = evidence.iter().map(|item| item.node_id.clone()).collect();
        let l2 = self.bus.l2_flow(
            l1,
            BusMessageKind::EvidenceRetrieved,
            "wiki-graph",
            "recipe-selector",
            "graph evidence ranked from atom and recipe relationships",
            &evidence_ids,
        );
        let recipe = select_recipe(&atoms, &evidence, self.provider.is_ready());
        let provider_result = if recipe.requires_provider || provider_requested(intent) {
            self.provider.prepare_call(intent, recipe.id, &evidence)
        } else {
            Ok(PreparedProviderCall {
                endpoint: String::new(),
                model: String::new(),
                api_key_env: String::new(),
                body: String::new(),
            })
        };

        let mut blockers = Vec::new();
        let provider_call = match provider_result {
            Ok(call) if !call.body.is_empty() => {
                self.bus.l2_flow(
                    l2,
                    BusMessageKind::ProviderPrepared,
                    "provider-adapter",
                    "model",
                    "provider request prepared from graph evidence",
                    &evidence_ids,
                );
                Some(call)
            }
            Ok(_) => None,
            Err(ProviderError::MissingApiKey { env }) => {
                blockers.push(format!("Missing provider credential in {env}"));
                self.bus.l2_flow(
                    l2,
                    BusMessageKind::ProviderBlocked,
                    "provider-adapter",
                    "operator",
                    "provider credential missing; route failed closed",
                    &evidence_ids,
                );
                None
            }
            Err(error) => {
                blockers.push(format!("Provider setup failed: {error:?}"));
                None
            }
        };

        if !evidence
            .iter()
            .any(|item| item.node_id == "mission:ornith-parity")
        {
            blockers.push("Ornith parity evidence was not retrieved".to_string());
        }

        let l3 = self.bus.l3_orchestrate(
            l2,
            BusMessageKind::RecipeSelected,
            "recipe-selector",
            "proof-loop",
            recipe.id,
            &evidence_ids,
        );
        if !self.bus.contains_all_layers() {
            blockers.push("Spiderweb Bus route did not touch all L0-L3 layers".to_string());
        }
        let status = if blockers.is_empty() {
            RuntimeStatus::Proven
        } else {
            RuntimeStatus::Blocked
        };
        let proof_kind = if blockers.is_empty() {
            BusMessageKind::ProofCaptured
        } else {
            BusMessageKind::ProofBlocked
        };
        let proof_body = if blockers.is_empty() {
            format!(
                "{} selected for {} with {} evidence nodes",
                recipe.name,
                mission().parity_floor,
                evidence.len()
            )
        } else {
            blockers.join("; ")
        };
        let proof = self.bus.l3_orchestrate(
            l3,
            proof_kind,
            "proof-loop",
            "artifact-state",
            &proof_body,
            &evidence_ids,
        );

        self.state.selected_recipe = recipe.id.to_string();
        self.state.selected_atoms = atoms.clone();
        self.state.status = status;
        self.state.evidence = evidence.clone();
        self.state.blockers = blockers.clone();
        self.state.last_provider_call = provider_call.clone();
        self.state.last_route = self.bus.route_for(proof).iter().map(|env| env.id).collect();
        if status == RuntimeStatus::Proven {
            self.state.proof_count += 1;
        }

        ProofRun {
            recipe_id: recipe.id.to_string(),
            atom_keys: atoms,
            evidence,
            provider_call,
            blockers,
            proof_envelope: proof,
            status,
        }
    }

    pub fn mark_drift(&mut self, reason: &str) {
        self.state.drift_count += 1;
        self.state.status = RuntimeStatus::DriftFlagged;
        self.bus.l3_orchestrate(
            self.state.last_route.last().copied().unwrap_or(0),
            BusMessageKind::DriftFlagged,
            "operator",
            "proof-loop",
            reason,
            &[],
        );
    }
}

fn classify_intent(intent: &str) -> Vec<String> {
    let lower = intent.to_ascii_lowercase();
    let mut scored: Vec<(&str, i32)> = crate::domain::atoms()
        .iter()
        .map(|atom| {
            let score = atom
                .keywords
                .iter()
                .filter(|keyword| lower.contains(**keyword))
                .count() as i32;
            (atom.key, score)
        })
        .filter(|(_, score)| *score > 0)
        .collect();
    scored.sort_by(|(a_key, a_score), (b_key, b_score)| {
        b_score.cmp(a_score).then_with(|| {
            atom_by_key(a_key)
                .unwrap()
                .id
                .cmp(&atom_by_key(b_key).unwrap().id)
        })
    });
    let mut atoms: Vec<String> = scored.into_iter().map(|(key, _)| key.to_string()).collect();
    if atoms.is_empty() {
        atoms = ["scan", "project", "compare", "compose", "measure"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    for required in ["flow", "preserve", "measure"] {
        if provider_requested(intent) && !atoms.iter().any(|key| key == required) {
            atoms.push(required.to_string());
        }
    }
    atoms.truncate(6);
    atoms
}

fn provider_requested(intent: &str) -> bool {
    let lower = intent.to_ascii_lowercase();
    ["provider", "api", "model", "openai", "llm", "rag"]
        .into_iter()
        .any(|term| lower.contains(term))
}

fn select_recipe(atoms: &[String], evidence: &[Evidence], provider_ready: bool) -> &'static Recipe {
    let mut candidates: Vec<(&Recipe, i32)> = recipes()
        .iter()
        .map(|recipe| {
            let atom_overlap = recipe
                .atoms
                .iter()
                .filter(|atom| atoms.iter().any(|key| key == **atom))
                .count() as i32;
            let evidence_score = evidence
                .iter()
                .filter(|item| item.node_id == recipe.id)
                .map(|item| item.score)
                .max()
                .unwrap_or(0);
            let provider_penalty = if recipe.requires_provider && !provider_ready {
                -30
            } else {
                0
            };
            let score = atom_overlap * 8 + evidence_score + provider_penalty - recipe.bonds as i32;
            (recipe, score)
        })
        .collect();
    candidates.sort_by(|(a_recipe, a_score), (b_recipe, b_score)| {
        b_score
            .cmp(a_score)
            .then_with(|| a_recipe.bonds.cmp(&b_recipe.bonds))
            .then_with(|| a_recipe.id.cmp(b_recipe.id))
    });
    candidates[0].0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::BusLayer;

    #[test]
    fn run_intent_routes_through_all_spiderweb_layers() {
        let mut runtime =
            MathAtomsRuntime::new(ProviderConfig::from_pairs(&[("OPENAI_API_KEY", "set")]));
        let run = runtime.run_intent("Build provider api wiki graph rag on the spiderweb bus");
        assert_eq!(run.status, RuntimeStatus::Proven);
        assert!(runtime.bus().contains_layer(BusLayer::L0Transport));
        assert!(runtime.bus().contains_layer(BusLayer::L1Message));
        assert!(runtime.bus().contains_layer(BusLayer::L2Flow));
        assert!(runtime.bus().contains_layer(BusLayer::L3Orchestration));
        assert!(run.provider_call.is_some());
    }

    #[test]
    fn missing_provider_fails_closed() {
        let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_pairs(&[]));
        let run = runtime.run_intent("Run the model provider api against graph evidence");
        assert_eq!(run.status, RuntimeStatus::Blocked);
        assert!(run
            .blockers
            .iter()
            .any(|item| item.contains("OPENAI_API_KEY")));
        assert!(runtime
            .bus()
            .envelopes()
            .iter()
            .any(|env| env.kind == BusMessageKind::ProviderBlocked));
    }

    #[test]
    fn ornith_is_the_product_mission() {
        let m = mission();
        assert!(m.body.contains("Ornith 1.0"));
        assert_eq!(m.parity_floor, "Ornith 1.0");
    }
}
