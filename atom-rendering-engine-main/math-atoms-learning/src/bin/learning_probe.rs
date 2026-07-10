use math_atoms_learning::{
    artifact_hash, effective_records, rank_records, LearningOutcome, LearningRecord,
    LearningRecordInput, LearningStore, LearningSummary, DEFAULT_GRAPH_MEMORY_LIMIT,
};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("learning probe blocked: {reason}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("record") => record(&args[1..]),
        Some("summary") => summary(&args[1..]),
        Some("context") => context(&args[1..]),
        _ => Err("expected record, summary, or context command".to_string()),
    }
}

fn record(args: &[String]) -> Result<(), String> {
    let store = store_from(args);
    let intent = read_required_file(args, "--intent-file")?;
    let outcome_raw = required(args, "--outcome")?;
    let outcome = LearningOutcome::parse(&outcome_raw)
        .ok_or_else(|| format!("invalid learning outcome: {outcome_raw}"))?;
    let artifact_path = optional(args, "--artifact").unwrap_or_default();
    let supplied_hash = optional(args, "--artifact-hash").unwrap_or_default();
    let artifact_hash = if supplied_hash.is_empty() && !artifact_path.is_empty() {
        artifact_hash(&artifact_path)
            .map_err(|error| format!("artifact hash failed for {artifact_path}: {error}"))?
    } else {
        supplied_hash
    };
    let input = LearningRecordInput {
        source: required(args, "--source")?,
        intent,
        recipe_id: optional(args, "--recipe").unwrap_or_default(),
        atom_stack: optional(args, "--atoms")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        gate: required(args, "--gate")?,
        attempt: parse_value(args, "--attempt")?,
        outcome,
        failure: read_optional_file(args, "--failure-file")?,
        correction: read_optional_file(args, "--correction-file")?,
        artifact_path,
        artifact_hash,
        provider_model: optional(args, "--provider-model").unwrap_or_default(),
        work_plan_id: optional(args, "--work-plan-id").unwrap_or_default(),
        work_plan_manifest: optional(args, "--work-plan-manifest").unwrap_or_default(),
        work_packet_count: parse_optional(args, "--work-packet-count")?.unwrap_or(0),
        route_len: parse_optional(args, "--route-len")?.unwrap_or(0),
    };
    let record = LearningRecord::new(input);
    store.append(&record).map_err(|error| {
        format!(
            "learning append failed at {}: {error}",
            store.path().display()
        )
    })?;
    let records = store
        .read_records()
        .map_err(|error| format!("learning readback failed: {error}"))?;
    if !records.iter().any(|persisted| persisted.id == record.id) {
        return Err("learning append readback did not contain the written record".to_string());
    }
    println!(
        "MATH_ATOMS_LEARNING_OK id={} outcome={} total={}",
        record.id,
        record.outcome.as_str(),
        records.len()
    );
    Ok(())
}

fn summary(args: &[String]) -> Result<(), String> {
    let store = store_from(args);
    let records = store
        .read_records()
        .map_err(|error| format!("learning summary read failed: {error}"))?;
    let summary = LearningSummary::from_records(&records);
    println!(
        "MATH_ATOMS_LEARNING_SUMMARY total={} failed={} succeeded={} path={}",
        summary.total,
        summary.failed,
        summary.succeeded,
        store.path().display()
    );
    Ok(())
}

fn context(args: &[String]) -> Result<(), String> {
    let store = store_from(args);
    let records = store
        .read_records()
        .map_err(|error| format!("learning context read failed: {error}"))?;
    let memory = effective_records(&records, DEFAULT_GRAPH_MEMORY_LIMIT);
    let intent = read_required_file(args, "--intent-file")?;
    let atoms: Vec<String> = optional(args, "--atoms")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let limit = parse_optional(args, "--limit")?.unwrap_or(8usize);
    let hits = rank_records(&memory, &intent, &atoms, limit);
    println!("MATH_ATOMS_LEARNING_CONTEXT hits={}", hits.len());
    for hit in hits {
        println!("{} | {} | {}", hit.score, hit.title, hit.excerpt);
    }
    Ok(())
}

fn store_from(args: &[String]) -> LearningStore {
    optional(args, "--store")
        .map(LearningStore::new)
        .unwrap_or_else(|| LearningStore::new(LearningStore::default_path()))
}

fn required(args: &[String], name: &str) -> Result<String, String> {
    optional(args, name).ok_or_else(|| format!("missing required argument {name}"))
}

fn optional(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_value<T>(args: &[String], name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    required(args, name)?
        .parse()
        .map_err(|_| format!("invalid value for {name}"))
}

fn parse_optional<T>(args: &[String], name: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
{
    optional(args, name)
        .map(|value| {
            value
                .parse()
                .map_err(|_| format!("invalid value for {name}"))
        })
        .transpose()
}

fn read_required_file(args: &[String], name: &str) -> Result<String, String> {
    let path = PathBuf::from(required(args, name)?);
    fs::read_to_string(&path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn read_optional_file(args: &[String], name: &str) -> Result<String, String> {
    let Some(path) = optional(args, name) else {
        return Ok(String::new());
    };
    fs::read_to_string(&path).map_err(|error| format!("could not read {path}: {error}"))
}
