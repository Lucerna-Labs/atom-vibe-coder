pub type EnvelopeId = u64;
pub type ThreadId = u64;

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
pub enum TransferPolicy {
    SharedNode,
    Backpressure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusMessageKind {
    IntentIngress,
    IntentClassified,
    RetrievalRequested,
    EvidenceRetrieved,
    ProviderPrepared,
    ProviderExecutionRequested,
    ProviderExecutionScheduled,
    ProviderExecuted,
    ProviderBlocked,
    RecipeSelected,
    ProofCaptured,
    ProofPending,
    ProofBlocked,
    StoreLearned,
    StoreObserved,
    StoreBlocked,
    LearningObserved,
    LearningPersisted,
    LearningApplied,
    DriftFlagged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Envelope {
    pub id: EnvelopeId,
    pub parent: Option<EnvelopeId>,
    pub thread_id: ThreadId,
    pub layer: BusLayer,
    pub ramp: Ramp,
    pub kind: BusMessageKind,
    pub source: String,
    pub target: String,
    pub body: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FabricThread {
    pub id: ThreadId,
    pub envelopes: Vec<EnvelopeId>,
    pub nodes: Vec<String>,
    pub layers: Vec<BusLayer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Intersection {
    pub node: String,
    pub threads: Vec<ThreadId>,
    pub transfer: TransferPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreloadPlan {
    pub thread_id: ThreadId,
    pub from_envelope: EnvelopeId,
    pub targets: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackpressureSignal {
    pub thread_id: ThreadId,
    pub envelope: EnvelopeId,
    pub layer: BusLayer,
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FabricSnapshot {
    pub threads: Vec<FabricThread>,
    pub intersections: Vec<Intersection>,
    pub preloads: Vec<PreloadPlan>,
    pub backpressure: Vec<BackpressureSignal>,
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
    next_thread_id: ThreadId,
    envelopes: Vec<Envelope>,
    threads: Vec<FabricThread>,
    intersections: Vec<Intersection>,
    preloads: Vec<PreloadPlan>,
    backpressure: Vec<BackpressureSignal>,
}

impl SpiderwebBus {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            next_thread_id: 1,
            envelopes: Vec::new(),
            threads: Vec::new(),
            intersections: Vec::new(),
            preloads: Vec::new(),
            backpressure: Vec::new(),
        }
    }

    pub fn l0_transport(
        &mut self,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
    ) -> EnvelopeId {
        self.l0_transport_from(None, kind, source, target, body)
    }

    pub fn l0_transport_from(
        &mut self,
        parent: Option<EnvelopeId>,
        kind: BusMessageKind,
        source: &str,
        target: &str,
        body: &str,
    ) -> EnvelopeId {
        self.emit(EnvelopeDraft {
            parent,
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

    pub fn threads(&self) -> &[FabricThread] {
        &self.threads
    }

    pub fn intersections(&self) -> &[Intersection] {
        &self.intersections
    }

    pub fn preloads(&self) -> &[PreloadPlan] {
        &self.preloads
    }

    pub fn backpressure(&self) -> &[BackpressureSignal] {
        &self.backpressure
    }

    pub fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            threads: self.threads.clone(),
            intersections: self.intersections.clone(),
            preloads: self.preloads.clone(),
            backpressure: self.backpressure.clone(),
        }
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
        let thread_id = self
            .thread_for_parent(draft.parent)
            .unwrap_or_else(|| self.create_thread());
        self.envelopes.push(Envelope {
            id,
            parent: draft.parent,
            thread_id,
            layer: draft.layer,
            ramp: draft.ramp,
            kind: draft.kind,
            source: draft.source.to_string(),
            target: draft.target.to_string(),
            body: draft.body.to_string(),
            evidence_ids: draft.evidence_ids.to_vec(),
        });
        self.record_fabric(id, thread_id, &draft);
        id
    }

    fn thread_for_parent(&self, parent: Option<EnvelopeId>) -> Option<ThreadId> {
        let parent = parent?;
        self.envelopes
            .iter()
            .find(|env| env.id == parent)
            .map(|env| env.thread_id)
    }

    fn create_thread(&mut self) -> ThreadId {
        let id = self.next_thread_id;
        self.next_thread_id += 1;
        self.threads.push(FabricThread {
            id,
            envelopes: Vec::new(),
            nodes: Vec::new(),
            layers: Vec::new(),
        });
        id
    }

    fn record_fabric(&mut self, id: EnvelopeId, thread_id: ThreadId, draft: &EnvelopeDraft<'_>) {
        let endpoints = [draft.source, draft.target];
        let mut crossing_threads = Vec::new();
        for thread in &self.threads {
            if thread.id == thread_id {
                continue;
            }
            if endpoints
                .iter()
                .any(|endpoint| thread.nodes.iter().any(|node| node == endpoint))
            {
                push_unique_u64(&mut crossing_threads, thread.id);
            }
        }

        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            thread.envelopes.push(id);
            push_unique_layer(&mut thread.layers, draft.layer);
            for endpoint in endpoints {
                push_unique_string(&mut thread.nodes, endpoint);
            }
        }

        for endpoint in endpoints {
            let mut threads = crossing_threads.clone();
            push_unique_u64(&mut threads, thread_id);
            if threads.len() > 1 {
                self.add_intersection(endpoint, threads, transfer_for(draft.kind));
            }
        }

        if matches!(draft.layer, BusLayer::L2Flow | BusLayer::L3Orchestration) {
            let mut targets = draft.evidence_ids.to_vec();
            if targets.is_empty() {
                targets.push(draft.target.to_string());
            }
            self.preloads.push(PreloadPlan {
                thread_id,
                from_envelope: id,
                targets,
            });
        }

        if is_blocking_kind(draft.kind) {
            self.backpressure.push(BackpressureSignal {
                thread_id,
                envelope: id,
                layer: draft.layer,
                source: draft.source.to_string(),
                reason: draft.body.to_string(),
            });
        }
    }

    fn add_intersection(
        &mut self,
        node: &str,
        mut threads: Vec<ThreadId>,
        transfer: TransferPolicy,
    ) {
        threads.sort_unstable();
        threads.dedup();
        if let Some(existing) = self
            .intersections
            .iter_mut()
            .find(|intersection| intersection.node == node && intersection.threads == threads)
        {
            if transfer == TransferPolicy::Backpressure {
                existing.transfer = transfer;
            }
            return;
        }
        self.intersections.push(Intersection {
            node: node.to_string(),
            threads,
            transfer,
        });
    }
}

fn is_blocking_kind(kind: BusMessageKind) -> bool {
    matches!(
        kind,
        BusMessageKind::ProviderBlocked
            | BusMessageKind::ProofBlocked
            | BusMessageKind::StoreBlocked
    )
}

fn transfer_for(kind: BusMessageKind) -> TransferPolicy {
    if is_blocking_kind(kind) {
        TransferPolicy::Backpressure
    } else {
        TransferPolicy::SharedNode
    }
}

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|item| item == value) {
        values.push(value.to_string());
    }
}

fn push_unique_layer(values: &mut Vec<BusLayer>, value: BusLayer) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn push_unique_u64(values: &mut Vec<u64>, value: u64) {
    if !values.contains(&value) {
        values.push(value);
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

    #[test]
    fn fabric_threads_form_as_messages_flow() {
        let mut bus = SpiderwebBus::new();
        let l0 = bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator",
            "runtime",
            "intent",
        );
        let l1 = bus.l1_message(
            l0,
            BusMessageKind::IntentClassified,
            "runtime",
            "wiki-graph",
            "atoms",
        );
        let evidence = vec!["mission:production-app-build".to_string()];
        let l2 = bus.l2_flow(
            l1,
            BusMessageKind::EvidenceRetrieved,
            "wiki-graph",
            "recipe-selector",
            "evidence",
            &evidence,
        );
        let route = bus.route_for(l2);

        assert_eq!(bus.threads().len(), 1);
        assert_eq!(route[0].thread_id, route[2].thread_id);
        assert!(bus.threads()[0]
            .nodes
            .iter()
            .any(|node| node == "wiki-graph"));
        assert!(bus
            .preloads()
            .iter()
            .any(|plan| plan.from_envelope == l2 && plan.targets == evidence));
    }

    #[test]
    fn intersections_emerge_when_threads_share_nodes() {
        let mut bus = SpiderwebBus::new();
        bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator-a",
            "shared-node",
            "a",
        );
        bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator-b",
            "shared-node",
            "b",
        );

        assert_eq!(bus.threads().len(), 2);
        assert!(bus.intersections().iter().any(|intersection| {
            intersection.node == "shared-node" && intersection.threads.len() == 2
        }));
    }

    #[test]
    fn blocking_routes_emit_backpressure_vibrations() {
        let mut bus = SpiderwebBus::new();
        let l0 = bus.l0_transport(
            BusMessageKind::IntentIngress,
            "operator",
            "runtime",
            "intent",
        );
        let l1 = bus.l1_message(
            l0,
            BusMessageKind::ProviderBlocked,
            "provider-adapter",
            "proof-loop",
            "provider returned 401",
        );

        assert!(bus
            .backpressure()
            .iter()
            .any(|signal| { signal.envelope == l1 && signal.reason == "provider returned 401" }));
        assert_eq!(
            bus.route_for(l1).last().map(|env| env.thread_id),
            bus.backpressure().last().map(|signal| signal.thread_id)
        );
    }
}
