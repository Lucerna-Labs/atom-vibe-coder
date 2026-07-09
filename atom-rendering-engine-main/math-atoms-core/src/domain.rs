#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomLayer {
    Root,
    Extended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Atom {
    pub id: u8,
    pub key: &'static str,
    pub layer: AtomLayer,
    pub summary: &'static str,
    pub currency: &'static str,
    pub keywords: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecipeStatus {
    Proven,
    Draft,
    Blocked,
}

impl RecipeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Draft => "draft",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recipe {
    pub id: &'static str,
    pub name: &'static str,
    pub level: &'static str,
    pub status: RecipeStatus,
    pub kind: &'static str,
    pub summary: &'static str,
    pub atoms: &'static [&'static str],
    pub bonds: u8,
    pub requires_provider: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gate {
    pub title: &'static str,
    pub body: &'static str,
    pub layer: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mission {
    pub title: &'static str,
    pub body: &'static str,
    pub readiness_floor: &'static str,
}

const ATOMS: &[Atom] = &[
    Atom {
        id: 1,
        key: "scan",
        layer: AtomLayer::Root,
        summary: "Visit each element.",
        currency: "Coverage over a finite set.",
        keywords: &[
            "read",
            "inspect",
            "visit",
            "list",
            "inventory",
            "scan",
            "parse",
        ],
    },
    Atom {
        id: 2,
        key: "hash",
        layer: AtomLayer::Root,
        summary: "Reduce to identity.",
        currency: "Stable identity or integrity proof.",
        keywords: &[
            "hash",
            "identity",
            "checksum",
            "fingerprint",
            "integrity",
            "proof",
        ],
    },
    Atom {
        id: 3,
        key: "fold",
        layer: AtomLayer::Root,
        summary: "Reduce many values to one.",
        currency: "Single result from many inputs.",
        keywords: &["reduce", "summarize", "total", "aggregate", "fold"],
    },
    Atom {
        id: 4,
        key: "project",
        layer: AtomLayer::Root,
        summary: "Take a slice or view.",
        currency: "Relevant surface without unrelated state.",
        keywords: &[
            "view", "slice", "select", "pane", "surface", "preview", "project",
        ],
    },
    Atom {
        id: 5,
        key: "scale",
        layer: AtomLayer::Root,
        summary: "Resize, quantize, or normalize.",
        currency: "Same intent at a new size or precision.",
        keywords: &[
            "resize",
            "normalize",
            "quantize",
            "scale",
            "fit",
            "responsive",
        ],
    },
    Atom {
        id: 6,
        key: "compare",
        layer: AtomLayer::Root,
        summary: "Decide equality or ordering.",
        currency: "Difference, ordering, or pass/fail decision.",
        keywords: &["compare", "diff", "decide", "rank", "gate", "verdict"],
    },
    Atom {
        id: 7,
        key: "combine",
        layer: AtomLayer::Root,
        summary: "Join two things.",
        currency: "Useful joined output.",
        keywords: &["join", "merge", "bind", "combine", "connect", "bond"],
    },
    Atom {
        id: 8,
        key: "order",
        layer: AtomLayer::Root,
        summary: "Establish sequence.",
        currency: "Correct ordering or run sequence.",
        keywords: &["order", "sequence", "sort", "route", "loop", "timeline"],
    },
    Atom {
        id: 9,
        key: "transform",
        layer: AtomLayer::Extended,
        summary: "Change basis or representation.",
        currency: "Representation changes become simpler and measurable.",
        keywords: &[
            "transform",
            "basis",
            "representation",
            "serialize",
            "rotation",
        ],
    },
    Atom {
        id: 10,
        key: "flow",
        layer: AtomLayer::Extended,
        summary: "Move value along a path.",
        currency: "Message or value route is explicit and testable.",
        keywords: &[
            "flow",
            "path",
            "lane",
            "transport",
            "propagate",
            "bus",
            "fabric",
        ],
    },
    Atom {
        id: 11,
        key: "preserve",
        layer: AtomLayer::Extended,
        summary: "Conservation or invariance.",
        currency: "Named invariant survives a transformation.",
        keywords: &[
            "preserve",
            "invariant",
            "conserve",
            "budget",
            "stable",
            "required",
        ],
    },
    Atom {
        id: 12,
        key: "compose",
        layer: AtomLayer::Extended,
        summary: "Nested structure.",
        currency: "Higher-level operations become easier to inspect.",
        keywords: &[
            "compose",
            "nested",
            "molecule",
            "tree",
            "orchestrator",
            "recipe",
        ],
    },
    Atom {
        id: 13,
        key: "dual",
        layer: AtomLayer::Extended,
        summary: "Complementary or paired variables.",
        currency: "Pairing exposes useful reversible or complementary work.",
        keywords: &["dual", "paired", "forward", "backward", "encode", "decode"],
    },
    Atom {
        id: 14,
        key: "measure",
        layer: AtomLayer::Extended,
        summary: "Extract observable while affecting the system.",
        currency: "Observability cost is named and bounded.",
        keywords: &[
            "measure",
            "telemetry",
            "observe",
            "profile",
            "log",
            "capture",
            "proof",
        ],
    },
    Atom {
        id: 15,
        key: "symmetrize",
        layer: AtomLayer::Extended,
        summary: "Enforce or exploit invariance.",
        currency: "Symmetry reduces work or rejects drift.",
        keywords: &["symmetry", "symmetrize", "mirror", "rotation", "parallel"],
    },
    Atom {
        id: 16,
        key: "superpose",
        layer: AtomLayer::Extended,
        summary: "Weighted blend with interference.",
        currency: "Interference matters beyond ordinary weighted combine.",
        keywords: &[
            "superpose",
            "interference",
            "attention",
            "weighted",
            "coherent",
        ],
    },
];

const RECIPES: &[Recipe] = &[
    Recipe {
        id: "native-atom-renderer",
        name: "Native Atom Renderer",
        level: "L2",
        status: RecipeStatus::Proven,
        kind: "renderer",
        summary: "PMRE-rendered local surface with no browser runtime dependency.",
        atoms: &["project", "combine", "measure", "compose"],
        bonds: 2,
        requires_provider: false,
    },
    Recipe {
        id: "spiderweb-proof-loop",
        name: "Spiderweb Proof Loop",
        level: "L3",
        status: RecipeStatus::Proven,
        kind: "fabric",
        summary: "L0-L3 route with ingress, message, flow, orchestration, evidence, and proof capture.",
        atoms: &["scan", "flow", "preserve", "order", "compare", "measure"],
        bonds: 4,
        requires_provider: false,
    },
    Recipe {
        id: "wiki-graph-rag",
        name: "Wiki Graph RAG",
        level: "L2",
        status: RecipeStatus::Proven,
        kind: "retrieval",
        summary: "Graph-first retrieval over atom, recipe, provider, proof, and renderer evidence.",
        atoms: &["scan", "hash", "project", "compare", "order"],
        bonds: 3,
        requires_provider: false,
    },
    Recipe {
        id: "provider-model-loop",
        name: "Provider Model Loop",
        level: "L1",
        status: RecipeStatus::Proven,
        kind: "provider",
        summary: "Prepare an OpenAI Responses API model call from current graph evidence and proof state.",
        atoms: &["measure", "compose", "flow", "preserve"],
        bonds: 3,
        requires_provider: true,
    },
    Recipe {
        id: "production-app-runtime",
        name: "Production App Runtime",
        level: "L3",
        status: RecipeStatus::Proven,
        kind: "product",
        summary: "Select the smallest gate-passing native recipe that matches the requested app behavior.",
        atoms: &["scan", "compare", "compose", "measure", "preserve", "order"],
        bonds: 5,
        requires_provider: true,
    },
];

const GATES: &[Gate] = &[
    Gate {
        title: "Recipe first",
        body: "Local atom recipes and graph evidence are tried before generic model text.",
        layer: "L3",
    },
    Gate {
        title: "Proof required",
        body: "No done state without current evidence and a recorded bus route.",
        layer: "L1",
    },
    Gate {
        title: "Artifact visible",
        body: "The native PMRE artifact must reflect the latest run state.",
        layer: "L2",
    },
    Gate {
        title: "Fail closed",
        body: "Unsupported or unconfigured provider paths become blockers.",
        layer: "L3",
    },
    Gate {
        title: "App fit",
        body: "The selected route must match the requested app behavior instead of an unrelated benchmark.",
        layer: "L3",
    },
];

const MISSION: Mission = Mission {
    title: "Production App Build",
    body: "Build the requested app through native atom rendering, Spiderweb Bus routing, graph evidence, provider calls when required, and proof artifacts that match the actual user request.",
    readiness_floor: "requested app behavior",
};

pub fn atoms() -> &'static [Atom] {
    ATOMS
}

pub fn atom_by_key(key: &str) -> Option<&'static Atom> {
    ATOMS.iter().find(|atom| atom.key == key)
}

pub fn recipes() -> &'static [Recipe] {
    RECIPES
}

pub fn gates() -> &'static [Gate] {
    GATES
}

pub fn mission() -> Mission {
    MISSION
}
