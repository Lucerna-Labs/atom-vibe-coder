pub type EnvelopeId = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusLayer {
    L0Transport,
    L1Message,
    L2Flow,
    L3Orchestration,
}

impl BusLayer {
    pub fn label(self) -> &'static str {
        match self {
            Self::L0Transport => "L0 transport",
            Self::L1Message => "L1 message",
            Self::L2Flow => "L2 flow",
            Self::L3Orchestration => "L3 orchestration",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ramp {
    Ingress,
    Internal,
    OffRamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusMessageKind {
    IntentIngress,
    IntentClassified,
    RetrievalRequested,
    EvidenceRetrieved,
    ProviderPrepared,
    ProviderExecuted,
    ProviderBlocked,
    RecipeSelected,
    ProofCaptured,
    ProofPending,
    ProofBlocked,
    StoreLearned,
    StoreObserved,
    StoreBlocked,
    DriftFlagged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Envelope {
    pub id: EnvelopeId,
    pub parent: Option<EnvelopeId>,
    pub layer: BusLayer,
    pub ramp: Ramp,
    pub kind: BusMessageKind,
    pub source: String,
    pub target: String,
    pub body: String,
    pub evidence_ids: Vec<String>,
}

struct EnvelopeDraft<'a> {
    parent: Option<EnvelopeId>,
    layer: BusLayer,
    ramp: Ramp,
    kind: BusMessageKind,
    source: &'a str,
    target: &'a str,
    body: &'a str,
    evidence_ids: &'a [String],
}

#[derive(Clone, Debug, Default)]
pub struct SpiderwebBus {
    next_id: EnvelopeId,
    envelopes: Vec<Envelope>,
}

impl SpiderwebBus {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            envelopes: Vec::new(),
        }
    }

    pub fn l0_transport(
        &mut self,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
    ) -> EnvelopeId {
        self.emit(EnvelopeDraft {
            parent: None,
            layer: BusLayer::L0Transport,
            ramp: Ramp::Ingress,
            kind,
            source,
            target,
            body,
            evidence_ids: &[],
        })
    }

    pub fn l1_message(
        &mut self,
        parent: EnvelopeId,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
    ) -> EnvelopeId {
        self.emit(EnvelopeDraft {
            parent: Some(parent),
            layer: BusLayer::L1Message,
            ramp: Ramp::Internal,
            kind,
            source,
            target,
            body,
            evidence_ids: &[],
        })
    }

    pub fn l2_flow(
        &mut self,
        parent: EnvelopeId,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
        evidence_ids: &[String],
    ) -> EnvelopeId {
        self.emit(EnvelopeDraft {
            parent: Some(parent),
            layer: BusLayer::L2Flow,
            ramp: Ramp::Internal,
            kind,
            source,
            target,
            body,
            evidence_ids,
        })
    }

    pub fn l3_orchestrate(
        &mut self,
        parent: EnvelopeId,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
        evidence_ids: &[String],
    ) -> EnvelopeId {
        self.emit(EnvelopeDraft {
            parent: Some(parent),
            layer: BusLayer::L3Orchestration,
            ramp: Ramp::OffRamp,
            kind,
            source,
            target,
            body,
            evidence_ids,
        })
    }

    pub fn envelopes(&self) -> &[Envelope] {
        &self.envelopes
    }

    pub fn contains_layer(&self, layer: BusLayer) -> bool {
        self.envelopes.iter().any(|env| env.layer == layer)
    }

    pub fn contains_all_layers(&self) -> bool {
        [
            BusLayer::L0Transport,
            BusLayer::L1Message,
            BusLayer::L2Flow,
            BusLayer::L3Orchestration,
        ]
        .into_iter()
        .all(|layer| self.contains_layer(layer))
    }

    pub fn route_contains_all_layers(&self, id: EnvelopeId) -> bool {
        let route = self.route_for(id);
        [
            BusLayer::L0Transport,
            BusLayer::L1Message,
            BusLayer::L2Flow,
            BusLayer::L3Orchestration,
        ]
        .into_iter()
        .all(|layer| route.iter().any(|env| env.layer == layer))
    }

    pub fn route_for(&self, id: EnvelopeId) -> Vec<&Envelope> {
        let mut route = Vec::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            if let Some(env) = self.envelopes.iter().find(|env| env.id == current) {
                route.push(env);
                cursor = env.parent;
            } else {
                break;
            }
        }
        route.reverse();
        route
    }

    fn emit(&mut self, draft: EnvelopeDraft<'_>) -> EnvelopeId {
        let id = self.next_id;
        self.next_id += 1;
        self.envelopes.push(Envelope {
            id,
            parent: draft.parent,
            layer: draft.layer,
            ramp: draft.ramp,
            kind: draft.kind,
            source: draft.source.to_string(),
            target: draft.target.to_string(),
            body: draft.body.to_string(),
            evidence_ids: draft.evidence_ids.to_vec(),
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_check_is_route_specific_not_historical() {
        let mut bus = SpiderwebBus::new();
        let l0 = bus.l0_transport(BusMessageKind::IntentIngress, "a", "b", "intent");
        let l1 = bus.l1_message(l0, BusMessageKind::IntentClassified, "b", "c", "atoms");
        let l2 = bus.l2_flow(
            l1,
            BusMessageKind::EvidenceRetrieved,
            "c",
            "d",
            "evidence",
            &[],
        );
        let l3 = bus.l3_orchestrate(l2, BusMessageKind::RecipeSelected, "d", "e", "recipe", &[]);
        let orphan_l3 =
            bus.l3_orchestrate(0, BusMessageKind::ProofBlocked, "x", "y", "orphan", &[]);

        assert!(bus.contains_all_layers());
        assert!(bus.route_contains_all_layers(l3));
        assert!(!bus.route_contains_all_layers(orphan_l3));
    }
}
