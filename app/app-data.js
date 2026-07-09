window.MATH_ATOMS_DATA = {
  defaultIntent: "Build a tiny local app with an atom renderer, a self-correcting proof loop, a recipe-first store, and a live artifact pane.",
  operatorMission: {
    title: "Production App Build",
    body: "Build the requested app with native atom rendering, Spiderweb Bus routing, wiki graph RAG, provider execution when required, and proof capture.",
    recipes: [
      { key: "NATIVE", size: 1, gate: "passing" },
      { key: "BUS", size: 2, gate: "passing" },
      { key: "RAG", size: 3, gate: "passing" },
      { key: "API", size: 4, gate: "blocked" },
      { key: "PROOF", size: 5, gate: "passing" },
      { key: "APP", size: 6, gate: "pending" }
    ]
  },
  atoms: [
    {
      id: 1,
      key: "scan",
      layer: "root",
      summary: "Visit each element.",
      currency: "Coverage over a finite set.",
      keywords: ["read", "inspect", "visit", "list", "inventory", "scan", "parse"]
    },
    {
      id: 2,
      key: "hash",
      layer: "root",
      summary: "Reduce to identity.",
      currency: "Stable identity or integrity proof.",
      keywords: ["hash", "identity", "checksum", "fingerprint", "integrity", "proof"]
    },
    {
      id: 3,
      key: "fold",
      layer: "root",
      summary: "Reduce many values to one.",
      currency: "Single result from many inputs.",
      keywords: ["reduce", "summarize", "total", "aggregate", "fold"]
    },
    {
      id: 4,
      key: "project",
      layer: "root",
      summary: "Take a slice or view.",
      currency: "Relevant surface without unrelated state.",
      keywords: ["view", "slice", "select", "pane", "surface", "preview", "project"]
    },
    {
      id: 5,
      key: "scale",
      layer: "root",
      summary: "Resize, quantize, or normalize.",
      currency: "Same intent at a new size or precision.",
      keywords: ["resize", "normalize", "quantize", "scale", "fit", "responsive"]
    },
    {
      id: 6,
      key: "compare",
      layer: "root",
      summary: "Decide equality or ordering.",
      currency: "Difference, ordering, or pass/fail decision.",
      keywords: ["compare", "diff", "decide", "rank", "gate", "verdict"]
    },
    {
      id: 7,
      key: "combine",
      layer: "root",
      summary: "Join two things.",
      currency: "Useful joined output.",
      keywords: ["join", "merge", "bind", "combine", "connect", "bond"]
    },
    {
      id: 8,
      key: "order",
      layer: "root",
      summary: "Establish sequence.",
      currency: "Correct ordering or run sequence.",
      keywords: ["order", "sequence", "sort", "route", "loop", "timeline"]
    },
    {
      id: 9,
      key: "transform",
      layer: "extended",
      risk: "low",
      summary: "Change basis or representation.",
      currency: "Representation changes become simpler and measurable.",
      testIdea: "Formalize quantization block scaling as transform and compare codec clarity.",
      keywords: ["transform", "basis", "representation", "serialize", "rotation", "projection"]
    },
    {
      id: 10,
      key: "flow",
      layer: "extended",
      risk: "low",
      summary: "Move value along a path.",
      currency: "Message or value route is explicit and testable.",
      testIdea: "Model Spiderweb message propagation as flow and compare route explainability.",
      keywords: ["flow", "path", "lane", "transport", "propagate", "bus", "fabric"]
    },
    {
      id: 11,
      key: "preserve",
      layer: "extended",
      risk: "medium",
      summary: "Conservation or invariance.",
      currency: "Named invariant survives a transformation.",
      testIdea: "List subsystem invariants and identify which are actually preserved.",
      keywords: ["preserve", "invariant", "conserve", "budget", "stable", "required"]
    },
    {
      id: 12,
      key: "compose",
      layer: "extended",
      risk: "low",
      summary: "Nested structure.",
      currency: "Higher-level operations become easier to inspect.",
      testIdea: "Make compose first-class in the orchestrator API and compare recipe clarity.",
      keywords: ["compose", "nested", "molecule", "tree", "orchestrator", "recipe"]
    },
    {
      id: 13,
      key: "dual",
      layer: "extended",
      risk: "medium-high",
      summary: "Complementary or paired variables.",
      currency: "Pairing exposes useful reversible or complementary work.",
      testIdea: "Bench time-frequency duality in RF repair against a plain codec route.",
      keywords: ["dual", "paired", "forward", "backward", "encode", "decode", "frequency"]
    },
    {
      id: 14,
      key: "measure",
      layer: "extended",
      risk: "low",
      summary: "Extract observable while affecting the system.",
      currency: "Observability cost is named and bounded.",
      testIdea: "Compare telemetry routes with and without measure as a distinct atom.",
      keywords: ["measure", "telemetry", "observe", "profile", "log", "capture", "proof"]
    },
    {
      id: 15,
      key: "symmetrize",
      layer: "extended",
      risk: "low",
      summary: "Enforce or exploit invariance.",
      currency: "Symmetry reduces work or rejects drift.",
      testIdea: "Bench rotational symmetry in MM3E scene optimization.",
      keywords: ["symmetry", "symmetrize", "mirror", "rotation", "parallel", "roles"]
    },
    {
      id: 16,
      key: "superpose",
      layer: "extended",
      risk: "high",
      summary: "Weighted blend with interference.",
      currency: "Interference matters beyond ordinary weighted combine.",
      testIdea: "Decide whether attention needs superpose or only weighted combine.",
      keywords: ["superpose", "interference", "attention", "weighted", "coherent", "wave"]
    }
  ],
  gates: [
    { title: "Recipe first", body: "local atoms before generic code", layer: "L3" },
    { title: "Proof required", body: "no done state without current evidence", layer: "L1" },
    { title: "Artifact visible", body: "preview must reflect the run", layer: "L2" },
    { title: "Fail closed", body: "unsupported paths become blockers", layer: "L3" }
  ],
  recipes: [
    {
      id: "atom-renderer-bootstrap",
      name: "Atom Renderer Bootstrap",
      level: "L2",
      status: "proven",
      kind: "renderer",
      summary: "Tiny visual surface every generated app can mount.",
      atoms: ["project", "combine", "measure", "compose"],
      bonds: 2
    },
    {
      id: "atom-3d-scene-recipe",
      name: "Atom 3D Scene Recipe",
      level: "L2",
      status: "draft",
      kind: "renderer",
      summary: "Scene recipe with explicit geometry and proof surface.",
      atoms: ["transform", "scale", "symmetrize"],
      bonds: 1
    },
    {
      id: "proof-loop-harness",
      name: "Proof Loop Harness",
      level: "L1",
      status: "proven",
      kind: "proof",
      summary: "Current evidence gate for generated artifacts.",
      atoms: ["scan", "hash", "compare", "measure"],
      bonds: 3
    },
    {
      id: "spiderweb-fabric-route",
      name: "Spiderweb Fabric Route",
      level: "L3",
      status: "draft",
      kind: "fabric",
      summary: "Layered route with ramps, vibrations, and verdict capture.",
      atoms: ["flow", "preserve", "order", "compose"],
      bonds: 4
    },
    {
      id: "production-app-runtime",
      name: "Production App Runtime",
      level: "L3",
      status: "draft",
      kind: "bench",
      summary: "Select the smallest gate-passing native recipe that matches the requested app behavior.",
      atoms: ["scan", "compare", "scale", "preserve", "measure", "order"],
      bonds: 5
    }
  ],
  steps: [
    { id: "intent-atom", title: "Intent Atom", body: "Normalize the request into atoms, bonds, and molecules." },
    { id: "recipe-retrieval", title: "Recipe Retrieval", body: "Rank proven local atoms and reject generic fallback." },
    { id: "molecule-build", title: "Molecule Build", body: "Compose renderer, store, hooks, and harness routes." },
    { id: "proof-run", title: "Proof Run", body: "Run checks, surface failures, and capture evidence." },
    { id: "self-critique", title: "Self Critique", body: "Find drift, missing proof, stale state, and weak atoms." },
    { id: "patch-reaction", title: "Patch Reaction", body: "Apply the smallest correction and rebind the graph." },
    { id: "artifact-preview", title: "Artifact Preview", body: "Refresh the side artifact as a live proof surface." },
    { id: "store-learning", title: "Store Learning", body: "Capture the passing route as a reusable recipe." }
  ],
  fabric: {
    layers: [
      { id: "L0", label: "L0 transport", y: 70 },
      { id: "L1", label: "L1 message", y: 170 },
      { id: "L2", label: "L2 flow", y: 270 },
      { id: "L3", label: "L3 orchestration", y: 370 }
    ],
    nodes: [
      { id: "proof-capture", label: "Proof Capture", layer: "L1", x: 70, y: 170, color: "#db5a39" },
      { id: "atom-renderer", label: "Atom Renderer", layer: "L2", x: 70, y: 270, color: "#d89a00" },
      { id: "atom-3d", label: "Atom 3D", layer: "L2", x: 180, y: 270, color: "#d89a00" },
      { id: "self-correcting", label: "Self Correcting", layer: "L3", x: 70, y: 370, color: "#4558a8" },
      { id: "recipe-first", label: "Recipe First", layer: "L3", x: 180, y: 370, color: "#4558a8" },
      { id: "hybrid-db", label: "Hybrid DB", layer: "L1", x: 315, y: 130, color: "#db5a39" },
      { id: "anti-drift", label: "Anti Drift", layer: "L3", x: 315, y: 370, color: "#4558a8" },
      { id: "side-artifact", label: "Side Artifact", layer: "L2", x: 425, y: 255, color: "#d89a00" },
      { id: "gpu-compatible", label: "GPU Compatible", layer: "L0", x: 425, y: 55, color: "#008a94" }
    ],
    links: [
      ["proof-capture", "atom-renderer"],
      ["atom-renderer", "self-correcting"],
      ["proof-capture", "hybrid-db"],
      ["proof-capture", "side-artifact"],
      ["atom-renderer", "atom-3d"],
      ["atom-3d", "side-artifact"],
      ["recipe-first", "hybrid-db"],
      ["recipe-first", "anti-drift"],
      ["anti-drift", "side-artifact"],
      ["self-correcting", "anti-drift"],
      ["hybrid-db", "gpu-compatible"],
      ["atom-3d", "gpu-compatible"],
      ["proof-capture", "gpu-compatible"]
    ]
  }
};
