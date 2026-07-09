use crate::domain::{atoms, gates, mission, recipes};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub node_id: String,
    pub title: String,
    pub excerpt: String,
    pub score: i32,
}

#[derive(Clone, Debug)]
struct WikiNode {
    id: &'static str,
    title: &'static str,
    excerpt: &'static str,
    tags: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug)]
struct WikiEdge {
    from: &'static str,
    to: &'static str,
    weight: i32,
}

#[derive(Clone, Debug)]
pub struct WikiGraph {
    nodes: Vec<WikiNode>,
    edges: Vec<WikiEdge>,
}

impl WikiGraph {
    pub fn seeded() -> Self {
        let mut nodes = vec![
            WikiNode {
                id: "mission:ornith-parity",
                title: mission().title,
                excerpt: mission().body,
                tags: vec!["ornith", "parity", "mission", "product", "proof"],
            },
            WikiNode {
                id: "bus:spiderweb",
                title: "Spiderweb Bus",
                excerpt: "Messages move through L0 transport, L1 message, L2 flow, and L3 orchestration with ramps, evidence, and off-ramps.",
                tags: vec!["spiderweb", "bus", "fabric", "flow", "transport", "orchestration"],
            },
            WikiNode {
                id: "rag:wiki-graph",
                title: "Wiki Graph RAG",
                excerpt: "Retrieval starts from atom and recipe graph relationships before text excerpts are used as supporting evidence.",
                tags: vec!["wiki", "graph", "rag", "retrieval", "evidence", "relationship"],
            },
            WikiNode {
                id: "provider:openai-responses",
                title: "Provider API",
                excerpt: "OpenAI Responses API calls are prepared from current graph evidence and must fail closed when credentials are absent.",
                tags: vec!["provider", "api", "model", "openai", "responses", "credential"],
            },
            WikiNode {
                id: "renderer:pmre-native",
                title: "Native Atom Renderer",
                excerpt: "PMRE renders the product as local mathematical primitives without Chrome, Electron, Tauri, or browser-local state.",
                tags: vec!["renderer", "pmre", "native", "artifact", "atom", "no-browser"],
            },
            WikiNode {
                id: "gate:fail-closed",
                title: "Fail Closed Gate",
                excerpt: "Missing provider credentials, stale evidence, and unsupported routes become blockers, not silent fallbacks.",
                tags: vec!["gate", "fail", "closed", "blocker", "provider", "evidence"],
            },
        ];

        for atom in atoms() {
            nodes.push(WikiNode {
                id: atom.key,
                title: atom.key,
                excerpt: atom.currency,
                tags: atom.keywords.to_vec(),
            });
        }
        for recipe in recipes() {
            nodes.push(WikiNode {
                id: recipe.id,
                title: recipe.name,
                excerpt: recipe.summary,
                tags: recipe.atoms.to_vec(),
            });
        }
        for gate in gates() {
            nodes.push(WikiNode {
                id: gate.title,
                title: gate.title,
                excerpt: gate.body,
                tags: vec![gate.layer, "gate", "proof"],
            });
        }

        let mut edges = vec![
            WikiEdge {
                from: "mission:ornith-parity",
                to: "ornith-parity-runtime",
                weight: 8,
            },
            WikiEdge {
                from: "mission:ornith-parity",
                to: "bus:spiderweb",
                weight: 7,
            },
            WikiEdge {
                from: "mission:ornith-parity",
                to: "renderer:pmre-native",
                weight: 7,
            },
            WikiEdge {
                from: "bus:spiderweb",
                to: "spiderweb-proof-loop",
                weight: 9,
            },
            WikiEdge {
                from: "rag:wiki-graph",
                to: "wiki-graph-rag",
                weight: 9,
            },
            WikiEdge {
                from: "provider:openai-responses",
                to: "provider-model-loop",
                weight: 9,
            },
            WikiEdge {
                from: "renderer:pmre-native",
                to: "native-atom-renderer",
                weight: 9,
            },
            WikiEdge {
                from: "gate:fail-closed",
                to: "provider-model-loop",
                weight: 5,
            },
        ];
        for recipe in recipes() {
            for atom in recipe.atoms {
                edges.push(WikiEdge {
                    from: atom,
                    to: recipe.id,
                    weight: 4,
                });
            }
        }

        Self { nodes, edges }
    }

    pub fn retrieve(&self, query: &str, atom_keys: &[String], limit: usize) -> Vec<Evidence> {
        let terms = tokenize(query);
        let mut scored = vec![0; self.nodes.len()];
        if let Some(idx) = self
            .nodes
            .iter()
            .position(|node| node.id == "mission:ornith-parity")
        {
            scored[idx] += 50;
        }
        for (idx, node) in self.nodes.iter().enumerate() {
            scored[idx] += direct_score(node, &terms, atom_keys);
        }

        let before = scored.clone();
        for edge in &self.edges {
            let from_score = self
                .nodes
                .iter()
                .position(|node| node.id == edge.from)
                .map(|idx| before[idx])
                .unwrap_or(0);
            if from_score > 0 {
                if let Some(to_idx) = self.nodes.iter().position(|node| node.id == edge.to) {
                    scored[to_idx] += from_score.min(10) + edge.weight;
                }
            }
        }

        let mut evidence: Vec<Evidence> = self
            .nodes
            .iter()
            .zip(scored)
            .filter(|(_, score)| *score > 0)
            .map(|(node, score)| Evidence {
                node_id: node.id.to_string(),
                title: node.title.to_string(),
                excerpt: node.excerpt.to_string(),
                score,
            })
            .collect();
        evidence.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        evidence.truncate(limit);
        evidence
    }
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn direct_score(node: &WikiNode, terms: &[String], atom_keys: &[String]) -> i32 {
    let mut score = 0;
    let title = node.title.to_ascii_lowercase();
    let excerpt = node.excerpt.to_ascii_lowercase();
    for term in terms {
        if node.id.eq_ignore_ascii_case(term) {
            score += 8;
        }
        if title.contains(term) {
            score += 5;
        }
        if excerpt.contains(term) {
            score += 2;
        }
        if node.tags.iter().any(|tag| tag.eq_ignore_ascii_case(term)) {
            score += 6;
        }
    }
    for atom in atom_keys {
        if node.id == atom || node.tags.iter().any(|tag| tag.eq_ignore_ascii_case(atom)) {
            score += 4;
        }
    }
    score
}
