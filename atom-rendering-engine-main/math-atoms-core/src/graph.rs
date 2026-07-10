use crate::domain::{atoms, gates, mission, recipes};
use math_atoms_hash::{sha256_file, valid_sha256_tag};
use math_atoms_learning::{LearningOutcome, LearningRecord, DEFAULT_GRAPH_MEMORY_LIMIT};
use math_atoms_proof::ProofRecord;
use std::collections::{hash_map::DefaultHasher, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub node_id: String,
    pub title: String,
    pub excerpt: String,
    pub score: i32,
}

#[derive(Clone, Debug)]
struct WikiNode {
    id: String,
    title: String,
    excerpt: String,
    tags: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct StaticEdge {
    from: &'static str,
    to: &'static str,
    weight: i32,
}

#[derive(Clone, Debug)]
struct WikiEdge {
    from: String,
    to: String,
    weight: i32,
}

#[derive(Clone, Debug)]
pub struct WikiGraph {
    nodes: Vec<WikiNode>,
    edges: Vec<WikiEdge>,
    learning_nodes: VecDeque<(String, String)>,
}

impl WikiGraph {
    pub fn seeded() -> Self {
        let mut nodes = vec![
            WikiNode {
                id: "mission:production-app-build".to_string(),
                title: mission().title.to_string(),
                excerpt: mission().body.to_string(),
                tags: tags(&["production", "app", "mission", "product", "proof"]),
            },
            WikiNode {
                id: "bus:spiderweb".to_string(),
                title: "Spiderweb Bus".to_string(),
                excerpt: "Messages move through L0 transport, L1 message, L2 flow, and L3 orchestration with ramps, evidence, and off-ramps.".to_string(),
                tags: tags(&["spiderweb", "bus", "fabric", "flow", "transport", "orchestration"]),
            },
            WikiNode {
                id: "rag:wiki-graph".to_string(),
                title: "Wiki Graph RAG".to_string(),
                excerpt: "Retrieval starts from atom and recipe graph relationships before text excerpts are used as supporting evidence.".to_string(),
                tags: tags(&["wiki", "graph", "rag", "retrieval", "evidence", "relationship"]),
            },
            WikiNode {
                id: "provider:openai-responses".to_string(),
                title: "Provider API".to_string(),
                excerpt: "OpenAI Responses API calls are prepared from current graph evidence and must fail closed when credentials are absent.".to_string(),
                tags: tags(&["provider", "api", "model", "openai", "responses", "credential"]),
            },
            WikiNode {
                id: "renderer:pmre-native".to_string(),
                title: "Native Atom Renderer".to_string(),
                excerpt: "PMRE renders the product as local mathematical primitives without Chrome, Electron, Tauri, or browser-local state.".to_string(),
                tags: tags(&["renderer", "pmre", "native", "artifact", "atom", "no-browser"]),
            },
            WikiNode {
                id: "gate:fail-closed".to_string(),
                title: "Fail Closed Gate".to_string(),
                excerpt: "Missing provider credentials, stale evidence, and unsupported routes become blockers, not silent fallbacks.".to_string(),
                tags: tags(&["gate", "fail", "closed", "blocker", "provider", "evidence"]),
            },
        ];

        for atom in atoms() {
            nodes.push(WikiNode {
                id: atom.key.to_string(),
                title: atom.key.to_string(),
                excerpt: atom.currency.to_string(),
                tags: tags(atom.keywords),
            });
        }
        for recipe in recipes() {
            nodes.push(WikiNode {
                id: recipe.id.to_string(),
                title: recipe.name.to_string(),
                excerpt: recipe.summary.to_string(),
                tags: tags(recipe.atoms),
            });
        }
        for gate in gates() {
            nodes.push(WikiNode {
                id: gate.title.to_string(),
                title: gate.title.to_string(),
                excerpt: gate.body.to_string(),
                tags: tags(&[gate.layer, "gate", "proof"]),
            });
        }

        let mut edges = static_edges(&[
            StaticEdge {
                from: "mission:production-app-build",
                to: "production-app-runtime",
                weight: 8,
            },
            StaticEdge {
                from: "mission:production-app-build",
                to: "bus:spiderweb",
                weight: 7,
            },
            StaticEdge {
                from: "mission:production-app-build",
                to: "renderer:pmre-native",
                weight: 7,
            },
            StaticEdge {
                from: "mission:production-app-build",
                to: "rag:wiki-graph",
                weight: 7,
            },
            StaticEdge {
                from: "mission:production-app-build",
                to: "provider:openai-responses",
                weight: 7,
            },
            StaticEdge {
                from: "bus:spiderweb",
                to: "spiderweb-proof-loop",
                weight: 9,
            },
            StaticEdge {
                from: "rag:wiki-graph",
                to: "wiki-graph-rag",
                weight: 9,
            },
            StaticEdge {
                from: "provider:openai-responses",
                to: "provider-model-loop",
                weight: 9,
            },
            StaticEdge {
                from: "renderer:pmre-native",
                to: "native-atom-renderer",
                weight: 9,
            },
            StaticEdge {
                from: "gate:fail-closed",
                to: "provider-model-loop",
                weight: 5,
            },
        ]);
        for recipe in recipes() {
            for atom in recipe.atoms {
                edges.push(WikiEdge {
                    from: (*atom).to_string(),
                    to: recipe.id.to_string(),
                    weight: 4,
                });
            }
        }

        Self {
            nodes,
            edges,
            learning_nodes: VecDeque::new(),
        }
    }

    pub fn from_markdown_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        let mut graph = Self::seeded();
        graph.load_markdown_dir(dir.as_ref())?;
        Ok(graph)
    }

    pub fn from_default_dirs() -> Self {
        for candidate in default_wiki_dirs() {
            if candidate.is_dir() {
                if let Ok(graph) = Self::from_markdown_dir(&candidate) {
                    return graph;
                }
            }
        }
        Self::seeded()
    }

    pub fn retrieve(&self, query: &str, atom_keys: &[String], limit: usize) -> Vec<Evidence> {
        let terms = tokenize(query);
        let mut scored = vec![0; self.nodes.len()];
        if let Some(idx) = self
            .nodes
            .iter()
            .position(|node| node.id == "mission:production-app-build")
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
                title: node.title.clone(),
                excerpt: node.excerpt.clone(),
                score,
            })
            .collect();
        evidence.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        evidence.truncate(limit);
        pin_mission_evidence(&mut evidence, self, limit);
        evidence
    }

    pub fn has_relationship_path(&self, from: &str, to: &str, max_depth: usize) -> bool {
        if from == to {
            return true;
        }
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([(from.to_string(), 0usize)]);
        while let Some((node, depth)) = queue.pop_front() {
            if !seen.insert(node.clone()) || depth >= max_depth {
                continue;
            }
            for edge in self
                .edges
                .iter()
                .filter(|edge| edge.from == node || edge.to == node)
            {
                let next = if edge.from == node {
                    edge.to.clone()
                } else {
                    edge.from.clone()
                };
                if next == to {
                    return true;
                }
                queue.push_back((next, depth + 1));
            }
        }
        false
    }

    pub fn add_proof_record(&mut self, record: &ProofRecord) {
        if !proof_record_is_positive_evidence(record) {
            return;
        }
        let record_atoms = authored_record_atoms(record);
        let id = proof_node_id(record, &record_atoms);
        if self.nodes.iter().any(|node| node.id == id) {
            return;
        }
        let mut node_tags = vec![
            "proof".to_string(),
            "store".to_string(),
            record.status.to_ascii_lowercase(),
            record.provider_state.to_ascii_lowercase(),
        ];
        if !record.provider_output_hash.is_empty() {
            node_tags.push("provider-output-audited".to_string());
            node_tags.push(record.provider_model.to_ascii_lowercase());
        }
        node_tags.extend(record_atoms.iter().map(|atom| atom.to_ascii_lowercase()));
        self.nodes.push(WikiNode {
            id: id.clone(),
            title: format!("Proof: {}", record.recipe_id),
            excerpt: format!(
                "{} proof for {} with {} evidence nodes, {} route envelopes, {} blockers, provider {}, output {} bytes {}.",
                record.status,
                record.recipe_id,
                record.evidence_count,
                record.route_len,
                record.blockers.len(),
                record.provider_state,
                record.provider_output_len,
                record.provider_output_hash
            ),
            tags: node_tags,
        });
        self.edges.push(WikiEdge {
            from: id.clone(),
            to: record.recipe_id.clone(),
            weight: 8,
        });
        self.edges.push(WikiEdge {
            from: record.recipe_id.clone(),
            to: id.clone(),
            weight: 5,
        });
        for atom in &record_atoms {
            self.edges.push(WikiEdge {
                from: atom.clone(),
                to: id.clone(),
                weight: 4,
            });
        }
    }

    pub fn add_proof_records(&mut self, records: &[ProofRecord]) {
        for record in records {
            self.add_proof_record(record);
        }
    }

    pub fn add_learning_record(&mut self, record: &LearningRecord) {
        if record.validate().is_err() {
            return;
        }
        let id = record.node_id();
        if self.nodes.iter().any(|node| node.id == id) {
            return;
        }
        let memory_key = record.memory_key();
        if let Some(index) = self
            .learning_nodes
            .iter()
            .position(|(_, key)| key == &memory_key)
        {
            if let Some((superseded, _)) = self.learning_nodes.remove(index) {
                self.remove_node(&superseded);
            }
        }
        self.nodes.push(WikiNode {
            id: id.clone(),
            title: record.title(),
            excerpt: record.excerpt(),
            tags: record.tags(),
        });
        if self.nodes.iter().any(|node| node.id == record.recipe_id) {
            self.edges.push(WikiEdge {
                from: record.recipe_id.clone(),
                to: id.clone(),
                weight: 5,
            });
            if record.outcome == LearningOutcome::Succeeded && record.is_promotable_success() {
                self.edges.push(WikiEdge {
                    from: id.clone(),
                    to: record.recipe_id.clone(),
                    weight: 6,
                });
            }
        }
        self.learning_nodes.push_back((id.clone(), memory_key));
        while self.learning_nodes.len() > DEFAULT_GRAPH_MEMORY_LIMIT {
            if let Some((evicted, _)) = self.learning_nodes.pop_front() {
                self.remove_node(&evicted);
            }
        }
        for atom in &record.atom_stack {
            if atom_by_graph_id(self, atom) {
                self.edges.push(WikiEdge {
                    from: atom.clone(),
                    to: id.clone(),
                    weight: 4,
                });
            }
        }
    }

    pub fn add_learning_records(&mut self, records: &[LearningRecord]) {
        for record in records {
            self.add_learning_record(record);
        }
    }

    fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|node| node.id != id);
        self.edges.retain(|edge| edge.from != id && edge.to != id);
    }

    fn load_markdown_dir(&mut self, dir: &Path) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.load_markdown_dir(&path)?;
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                self.add_markdown_file(&path)?;
            }
        }
        Ok(())
    }

    fn add_markdown_file(&mut self, path: &Path) -> io::Result<()> {
        let text = fs::read_to_string(path)?;
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("wiki-node");
        let id = format!("wiki:{}", slug(stem));
        let title = text
            .lines()
            .find_map(|line| line.strip_prefix("# ").map(str::trim))
            .unwrap_or(stem)
            .to_string();
        let mut node_tags = vec!["wiki".to_string()];
        for line in text.lines() {
            if let Some(raw) = line.strip_prefix("tags:") {
                for item in raw.split(',') {
                    let tag = item.trim();
                    if !tag.is_empty() {
                        node_tags.push(tag.to_ascii_lowercase());
                    }
                }
            }
        }
        let excerpt = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("tags:"))
            .unwrap_or("")
            .to_string();
        for link in wiki_links(&text) {
            self.edges.push(WikiEdge {
                from: id.clone(),
                to: link,
                weight: 6,
            });
        }
        self.nodes.push(WikiNode {
            id,
            title,
            excerpt,
            tags: node_tags,
        });
        Ok(())
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
        if node.id == *atom || node.tags.iter().any(|tag| tag.eq_ignore_ascii_case(atom)) {
            score += 4;
        }
    }
    score
}

fn atom_by_graph_id(graph: &WikiGraph, id: &str) -> bool {
    atoms().iter().any(|atom| atom.key == id) && graph.nodes.iter().any(|node| node.id == id)
}

fn tags(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn static_edges(edges: &[StaticEdge]) -> Vec<WikiEdge> {
    edges
        .iter()
        .map(|edge| WikiEdge {
            from: edge.from.to_string(),
            to: edge.to.to_string(),
            weight: edge.weight,
        })
        .collect()
}

fn default_wiki_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var("MATH_ATOMS_WIKI_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(PathBuf::from("knowledge/wiki"));
    dirs.push(PathBuf::from("../knowledge/wiki"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(engine) = exe.parent().and_then(Path::parent).and_then(Path::parent) {
            dirs.push(engine.join("..").join("knowledge").join("wiki"));
        }
    }
    dirs
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn wiki_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let link = after[..end].trim();
        if !link.is_empty() {
            links.push(link.to_string());
        }
        rest = &after[end + 2..];
    }
    links
}

fn authored_record_atoms(record: &ProofRecord) -> Vec<String> {
    let Some(recipe) = recipes()
        .iter()
        .find(|recipe| recipe.id == record.recipe_id)
    else {
        return Vec::new();
    };
    recipe
        .atoms
        .iter()
        .filter(|atom| record.atoms.iter().any(|record_atom| record_atom == **atom))
        .map(|atom| (*atom).to_string())
        .collect()
}

fn proof_node_id(record: &ProofRecord, record_atoms: &[String]) -> String {
    let mut hasher = DefaultHasher::new();
    record.recipe_id.hash(&mut hasher);
    record.status.hash(&mut hasher);
    record_atoms.hash(&mut hasher);
    record.evidence_count.hash(&mut hasher);
    record.blockers.hash(&mut hasher);
    record.provider_state.hash(&mut hasher);
    record.provider_model.hash(&mut hasher);
    record.provider_endpoint.hash(&mut hasher);
    record.provider_output_artifact.hash(&mut hasher);
    record.provider_output_hash.hash(&mut hasher);
    record.provider_output_len.hash(&mut hasher);
    record.route_len.hash(&mut hasher);
    format!("proof:{:016x}", hasher.finish())
}

fn proof_record_is_positive_evidence(record: &ProofRecord) -> bool {
    if record.status != "proven"
        || !record.blockers.is_empty()
        || record.evidence_count == 0
        || record.route_len < 4
    {
        return false;
    }
    let Some(recipe) = recipes()
        .iter()
        .find(|recipe| recipe.id == record.recipe_id)
    else {
        return false;
    };
    if !recipe.requires_provider {
        return true;
    }
    record.provider_state == "provider:ran"
        && !record.provider_model.trim().is_empty()
        && !record.provider_endpoint.trim().is_empty()
        && !record.provider_output_artifact.trim().is_empty()
        && valid_sha256_tag(&record.provider_output_hash)
        && sha256_file(&record.provider_output_artifact)
            .map(|actual| actual == record.provider_output_hash)
            .unwrap_or(false)
        && fs::metadata(&record.provider_output_artifact)
            .map(|metadata| metadata.len() == record.provider_output_len as u64)
            .unwrap_or(false)
        && record.provider_output_len > 0
}

fn pin_mission_evidence(evidence: &mut Vec<Evidence>, graph: &WikiGraph, limit: usize) {
    if limit == 0
        || evidence
            .iter()
            .any(|item| item.node_id == "mission:production-app-build")
    {
        return;
    }
    let Some(node) = graph
        .nodes
        .iter()
        .find(|node| node.id == "mission:production-app-build")
    else {
        return;
    };
    if evidence.len() >= limit {
        evidence.pop();
    }
    evidence.push(Evidence {
        node_id: node.id.clone(),
        title: node.title.clone(),
        excerpt: node.excerpt.clone(),
        score: 50,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use math_atoms_hash::sha256_file;
    use math_atoms_learning::{LearningRecordInput, LEARNING_SCHEMA_VERSION};

    fn provider_artifact(label: &str, text: &str) -> (PathBuf, String) {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-{label}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, text).unwrap();
        let hash = sha256_file(&path).unwrap();
        (path, hash)
    }

    fn failed_learning(index: usize, intent: String) -> LearningRecord {
        let mut record = LearningRecord::new(LearningRecordInput {
            source: "graph-test".to_string(),
            intent,
            recipe_id: "provider-model-loop".to_string(),
            atom_stack: vec!["measure".to_string(), "flow".to_string()],
            gate: format!("gate-{index}"),
            attempt: 1,
            outcome: LearningOutcome::Failed,
            failure: format!("failure-{index}"),
            correction: String::new(),
            artifact_path: String::new(),
            artifact_hash: String::new(),
            provider_model: String::new(),
            route_len: 4,
        });
        record.schema_version = LEARNING_SCHEMA_VERSION;
        record
    }

    #[test]
    fn markdown_wiki_nodes_are_retrieved() {
        let dir = std::env::temp_dir().join(format!(
            "math-atoms-wiki-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("provider-proof.md"),
            "# Provider Proof\ntags: provider, rag\nProvider calls must fail closed and cite graph evidence.\n[[provider-model-loop]]\n",
        )
        .unwrap();
        let graph = WikiGraph::from_markdown_dir(&dir).unwrap();
        fs::remove_dir_all(&dir).ok();
        let hits = graph.retrieve("provider proof rag", &["measure".to_string()], 12);
        assert!(hits
            .iter()
            .any(|item| item.node_id == "wiki:provider-proof"));
    }

    #[test]
    fn proof_records_become_retrievable_evidence() {
        let mut graph = WikiGraph::seeded();
        graph.add_proof_record(&ProofRecord {
            recipe_id: "wiki-graph-rag".to_string(),
            status: "proven".to_string(),
            atoms: vec!["scan".to_string(), "hash".to_string()],
            evidence_count: 9,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "fake-responsive-provider".to_string(),
            provider_endpoint: "http://127.0.0.1:1/v1/responses".to_string(),
            provider_output_artifact: String::new(),
            provider_output_hash: "fnv:1111111111111111".to_string(),
            provider_output_len: 18,
            route_len: 6,
        });
        let hits = graph.retrieve("stored proof wiki graph", &["scan".to_string()], 12);
        assert!(hits.iter().any(|item| item.node_id.starts_with("proof:")));
    }

    #[test]
    fn provider_proof_records_need_execution_audit_before_rag_promotion() {
        let mut graph = WikiGraph::seeded();
        graph.add_proof_record(&ProofRecord {
            recipe_id: "provider-model-loop".to_string(),
            status: "proven".to_string(),
            atoms: vec!["measure".to_string(), "flow".to_string()],
            evidence_count: 9,
            blockers: Vec::new(),
            provider_state: "provider:idle".to_string(),
            provider_model: String::new(),
            provider_endpoint: String::new(),
            provider_output_artifact: String::new(),
            provider_output_hash: String::new(),
            provider_output_len: 0,
            route_len: 6,
        });
        let hits = graph.retrieve(
            "tampered provider proof stored proof",
            &["flow".to_string()],
            12,
        );
        assert!(!hits.iter().any(|item| item.node_id.starts_with("proof:")));
    }

    #[test]
    fn audited_provider_proof_records_become_rag_evidence() {
        let mut graph = WikiGraph::seeded();
        let output = "provider proof";
        let (artifact, hash) = provider_artifact("audited-provider-proof", output);
        graph.add_proof_record(&ProofRecord {
            recipe_id: "provider-model-loop".to_string(),
            status: "proven".to_string(),
            atoms: vec!["measure".to_string(), "flow".to_string()],
            evidence_count: 9,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: artifact.to_string_lossy().to_string(),
            provider_output_hash: hash,
            provider_output_len: output.len(),
            route_len: 8,
        });
        let hits = graph.retrieve(
            "audited provider proof stored proof",
            &["flow".to_string()],
            12,
        );
        assert!(hits.iter().any(|item| item.node_id.starts_with("proof:")));
        fs::remove_file(artifact).ok();
    }

    #[test]
    fn tampered_provider_artifact_cannot_become_proof_evidence() {
        let output = "verified provider output";
        let (artifact, hash) = provider_artifact("tampered-provider-proof", output);
        let record = ProofRecord {
            recipe_id: "provider-model-loop".to_string(),
            status: "proven".to_string(),
            atoms: vec!["measure".to_string(), "flow".to_string()],
            evidence_count: 9,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: artifact.to_string_lossy().to_string(),
            provider_output_hash: hash,
            provider_output_len: output.len(),
            route_len: 8,
        };
        fs::write(&artifact, "tampered provider output").unwrap();
        let mut graph = WikiGraph::seeded();
        graph.add_proof_record(&record);
        assert!(!graph.nodes.iter().any(|node| node.id.starts_with("proof:")));
        fs::remove_file(artifact).ok();
    }

    #[test]
    fn proof_record_atoms_are_limited_to_authored_recipe_stack() {
        let mut graph = WikiGraph::seeded();
        let output = "provider proof";
        let (artifact, hash) = provider_artifact("atom-limited-provider-proof", output);
        graph.add_proof_record(&ProofRecord {
            recipe_id: "provider-model-loop".to_string(),
            status: "proven".to_string(),
            atoms: vec![
                "measure".to_string(),
                "compose".to_string(),
                "flow".to_string(),
                "preserve".to_string(),
                "combine".to_string(),
            ],
            evidence_count: 9,
            blockers: Vec::new(),
            provider_state: "provider:ran".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: artifact.to_string_lossy().to_string(),
            provider_output_hash: hash,
            provider_output_len: output.len(),
            route_len: 8,
        });

        let proof_node = graph
            .nodes
            .iter()
            .find(|node| node.id.starts_with("proof:"))
            .expect("audited provider proof should be promoted");
        assert!(!proof_node.tags.iter().any(|tag| tag == "combine"));
        assert!(!graph
            .edges
            .iter()
            .any(|edge| edge.from == "combine" && edge.to == proof_node.id));
        fs::remove_file(artifact).ok();
    }

    #[test]
    fn live_learning_graph_is_bounded_and_deduplicated() {
        let mut graph = WikiGraph::seeded();
        let base_nodes = graph.nodes.len();
        for index in 0..(DEFAULT_GRAPH_MEMORY_LIMIT + 17) {
            graph.add_learning_record(&failed_learning(index, format!("intent-{index}")));
        }
        assert_eq!(graph.learning_nodes.len(), DEFAULT_GRAPH_MEMORY_LIMIT);
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|node| node.id.starts_with("learning:"))
                .count(),
            DEFAULT_GRAPH_MEMORY_LIMIT
        );
        assert_eq!(graph.nodes.len(), base_nodes + DEFAULT_GRAPH_MEMORY_LIMIT);

        let original = failed_learning(999, "deduplicated intent".to_string());
        let mut replacement = original.clone();
        replacement.id = "replacement-learning-record".to_string();
        replacement.timestamp_ms += 1;
        graph.add_learning_record(&original);
        graph.add_learning_record(&replacement);
        assert!(!graph.nodes.iter().any(|node| node.id == original.node_id()));
        assert!(graph
            .nodes
            .iter()
            .any(|node| node.id == replacement.node_id()));
        assert_eq!(graph.learning_nodes.len(), DEFAULT_GRAPH_MEMORY_LIMIT);
    }

    #[test]
    fn mission_path_reaches_every_core_recipe() {
        let graph = WikiGraph::seeded();
        for recipe in recipes() {
            assert!(
                graph.has_relationship_path(recipe.id, "mission:production-app-build", 6),
                "{} is not linked to the mission graph",
                recipe.id
            );
        }
        assert!(!graph.has_relationship_path("missing-recipe", "mission:production-app-build", 6));
    }

    #[test]
    fn blocked_proof_records_do_not_become_positive_evidence() {
        let mut graph = WikiGraph::seeded();
        graph.add_proof_record(&ProofRecord {
            recipe_id: "provider-model-loop".to_string(),
            status: "blocked".to_string(),
            atoms: vec!["measure".to_string(), "flow".to_string()],
            evidence_count: 9,
            blockers: vec!["provider returned 401".to_string()],
            provider_state: "provider:blocked".to_string(),
            provider_model: "gpt-test".to_string(),
            provider_endpoint: "https://api.openai.com/v1/responses".to_string(),
            provider_output_artifact: String::new(),
            provider_output_hash: String::new(),
            provider_output_len: 0,
            route_len: 6,
        });
        let hits = graph.retrieve(
            "provider returned 401 stored proof",
            &["flow".to_string()],
            12,
        );
        assert!(!hits.iter().any(|item| item.node_id.starts_with("proof:")));
    }

    #[test]
    fn mission_evidence_stays_pinned_with_many_store_records() {
        let mut graph = WikiGraph::seeded();
        for idx in 0..24 {
            graph.add_proof_record(&ProofRecord {
                recipe_id: "provider-model-loop".to_string(),
                status: "blocked".to_string(),
                atoms: vec!["measure".to_string(), "flow".to_string()],
                evidence_count: idx + 1,
                blockers: Vec::new(),
                provider_state: "provider:blocked".to_string(),
                provider_model: String::new(),
                provider_endpoint: String::new(),
                provider_output_artifact: String::new(),
                provider_output_hash: String::new(),
                provider_output_len: 0,
                route_len: 5,
            });
        }
        let hits = graph.retrieve(
            "provider model wiki graph rag spiderweb evidence",
            &["measure".to_string(), "flow".to_string()],
            8,
        );
        assert!(hits
            .iter()
            .any(|item| item.node_id == "mission:production-app-build"));
    }
}
