use crate::domain::{atoms, gates, mission, recipes};
use std::fs;
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
}

impl WikiGraph {
    pub fn seeded() -> Self {
        let mut nodes = vec![
            WikiNode {
                id: "mission:ornith-parity".to_string(),
                title: mission().title.to_string(),
                excerpt: mission().body.to_string(),
                tags: tags(&["ornith", "parity", "mission", "product", "proof"]),
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
                from: "mission:ornith-parity",
                to: "ornith-parity-runtime",
                weight: 8,
            },
            StaticEdge {
                from: "mission:ornith-parity",
                to: "bus:spiderweb",
                weight: 7,
            },
            StaticEdge {
                from: "mission:ornith-parity",
                to: "renderer:pmre-native",
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

        Self { nodes, edges }
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
        evidence
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
