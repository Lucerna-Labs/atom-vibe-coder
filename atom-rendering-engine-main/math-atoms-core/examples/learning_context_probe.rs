use math_atoms_core::{LearningStore, MathAtomsRuntime, ProofStore, ProviderConfig};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let intent = args
        .first()
        .map(String::as_str)
        .unwrap_or("Build a Bluetooth driver from learned gate evidence");
    let learning_path = args
        .get(1)
        .expect("learning_context_probe requires a learning store path");
    let proof_path = std::env::temp_dir().join("math-atoms-context-probe-proofs.jsonl");
    let mut runtime = MathAtomsRuntime::with_stores(
        ProviderConfig::from_pairs(&[]),
        ProofStore::new(proof_path),
        LearningStore::new(learning_path),
    );
    let run = runtime.run_intent(intent);
    let hits: Vec<_> = run
        .evidence
        .iter()
        .filter(|item| item.node_id.starts_with("learning:"))
        .collect();
    assert!(
        !hits.is_empty(),
        "no durable learning evidence was retrieved"
    );
    println!("MATH_ATOMS_LEARNING_RETRIEVED {}", hits[0].title);
}
