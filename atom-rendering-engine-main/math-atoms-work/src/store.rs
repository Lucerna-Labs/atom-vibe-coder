use crate::model::{validate_files, MAX_PACKET_OUTPUT_BYTES};
use crate::{
    validate_packet_output, validate_secure_packet_output, CompletedPacket, PacketContract,
    WorkError, WorkPacket, WorkPlan, WorkStage, WORK_SCHEMA_VERSION,
};
use math_atoms_hash::{sha256_file, sha256_tagged, valid_sha256_tag};
use math_atoms_json::{parse as parse_json, JsonValue};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
static LEASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(
        desired_access: u32,
        inherit_handle: i32,
        process_id: u32,
    ) -> *mut std::ffi::c_void;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPacket {
    pub schema_version: u32,
    pub plan_id: String,
    pub packet_id: String,
    pub ordinal: usize,
    pub stage: String,
    pub contract: String,
    pub model: String,
    pub output_path: PathBuf,
    pub output_hash: String,
    pub output_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkPlanStore {
    root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedWorkPlan {
    pub plan_id: String,
    pub packet_count: usize,
    pub model: String,
    pub packet_ids: Vec<String>,
}

#[derive(Debug)]
pub struct WorkPlanLease {
    path: PathBuf,
    owner_token: String,
}

impl Drop for WorkPlanLease {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path)
            .map(|value| value == self.owner_token)
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn default_work_root() -> PathBuf {
    if let Some(path) = non_empty_env("MATH_ATOMS_WORK_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = non_empty_env("MATH_ATOMS_STORE_DIR") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("work-packets");
    }
    if let Some(path) = non_empty_env("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("MathAtomsCoder")
            .join("work-packets");
    }
    std::env::temp_dir()
        .join("MathAtomsCoder")
        .join("work-packets")
}

pub fn verify_work_plan_evidence(
    manifest_path: impl AsRef<Path>,
    expected_plan_id: &str,
    expected_packet_count: usize,
) -> Result<VerifiedWorkPlan, WorkError> {
    validate_id(expected_plan_id)?;
    if expected_packet_count == 0 {
        return Err(WorkError::InvalidPlan(
            "expected packet count must be positive".to_string(),
        ));
    }
    let manifest_path = manifest_path.as_ref().canonicalize()?;
    if manifest_path.file_name().and_then(|name| name.to_str()) != Some("plan-expanded.json") {
        return Err(WorkError::InvalidPlan(
            "work evidence must reference plan-expanded.json".to_string(),
        ));
    }
    let plan_dir = manifest_path
        .parent()
        .ok_or_else(|| WorkError::InvalidPlan("manifest has no plan directory".to_string()))?
        .canonicalize()?;
    let manifest_text = fs::read_to_string(&manifest_path)?;
    let manifest = parse_json(&manifest_text)
        .map_err(|error| WorkError::InvalidPlan(format!("work manifest JSON: {error}")))?;
    let object = manifest
        .as_object()
        .ok_or_else(|| WorkError::InvalidPlan("work manifest is not an object".to_string()))?;
    let expected_fields: HashSet<&str> = [
        "schema_version",
        "plan_id",
        "intent_hash",
        "recipe_id",
        "atom_stack",
        "fingerprint_hash",
        "packet_count",
        "expanded",
        "packets",
    ]
    .into_iter()
    .collect();
    let actual_fields: HashSet<&str> = object.iter().map(|(name, _)| name.as_str()).collect();
    let expanded = matches!(manifest.get("expanded"), Some(JsonValue::Bool(true)));
    if actual_fields != expected_fields
        || number(&manifest, "schema_version")? != u64::from(WORK_SCHEMA_VERSION)
        || string(&manifest, "plan_id")? != expected_plan_id
        || number(&manifest, "packet_count")? as usize != expected_packet_count
        || !expanded
    {
        return Err(WorkError::InvalidPlan(
            "expanded work manifest does not match claimed plan evidence".to_string(),
        ));
    }

    let packet_values = manifest
        .get("packets")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| WorkError::InvalidPlan("manifest packets must be an array".to_string()))?;
    if packet_values.len() != expected_packet_count {
        return Err(WorkError::InvalidPlan(
            "manifest packet descriptor count does not match".to_string(),
        ));
    }
    let descriptor_fields: HashSet<&str> = [
        "id",
        "ordinal",
        "stage",
        "contract",
        "file_path",
        "dependencies",
        "max_output_bytes",
    ]
    .into_iter()
    .collect();
    let mut descriptors = Vec::new();
    for value in packet_values {
        let object = value.as_object().ok_or_else(|| {
            WorkError::InvalidPlan("manifest packet descriptor is not an object".to_string())
        })?;
        let actual: HashSet<&str> = object.iter().map(|(name, _)| name.as_str()).collect();
        if actual != descriptor_fields {
            return Err(WorkError::InvalidPlan(
                "manifest packet descriptor fields are invalid".to_string(),
            ));
        }
        let stage_text = string(value, "stage")?;
        let contract_text = string(value, "contract")?;
        let max_output_bytes = number(value, "max_output_bytes")? as usize;
        let packet = WorkPacket {
            id: string(value, "id")?.to_string(),
            ordinal: number(value, "ordinal")? as usize,
            stage: WorkStage::parse(stage_text).ok_or_else(|| {
                WorkError::InvalidPlan(format!("unknown manifest stage {stage_text}"))
            })?,
            contract: PacketContract::parse(contract_text).ok_or_else(|| {
                WorkError::InvalidPlan(format!("unknown manifest contract {contract_text}"))
            })?,
            objective: String::new(),
            acceptance: Vec::new(),
            dependencies: strings(value, "dependencies")?,
            file: match string(value, "file_path")? {
                "" => None,
                path => Some(crate::WorkFile {
                    path: path.to_string(),
                    purpose: "manifest evidence".to_string(),
                    acceptance: vec!["manifest evidence".to_string()],
                }),
            },
            max_output_bytes,
        };
        if packet.id.is_empty()
            || packet.max_output_bytes == 0
            || packet.max_output_bytes > MAX_PACKET_OUTPUT_BYTES
        {
            return Err(WorkError::InvalidPlan(
                "manifest packet descriptor bounds are invalid".to_string(),
            ));
        }
        descriptors.push(packet);
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(&plan_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || name.starts_with("plan-")
        {
            continue;
        }
        records.push(StoredPacket::from_json(&fs::read_to_string(path)?)?);
    }
    if records.len() != expected_packet_count {
        return Err(WorkError::InvalidPlan(format!(
            "work packet evidence count mismatch: expected {expected_packet_count}, got {}",
            records.len()
        )));
    }
    records.sort_by_key(|record| record.ordinal);
    let model = records
        .first()
        .map(|record| record.model.clone())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| WorkError::InvalidPlan("work packet model is empty".to_string()))?;
    let mut packet_ids = HashSet::new();
    let mut ordered_packet_ids = Vec::new();
    let mut manifest_files = Vec::new();
    for (ordinal, record) in records.iter().enumerate() {
        let descriptor = descriptors.get(ordinal).ok_or_else(|| {
            WorkError::InvalidPlan(format!("missing descriptor for packet ordinal {ordinal}"))
        })?;
        if record.schema_version != WORK_SCHEMA_VERSION
            || record.plan_id != expected_plan_id
            || record.ordinal != ordinal
            || descriptor.ordinal != ordinal
            || record.packet_id != descriptor.id
            || record.stage != descriptor.stage.as_str()
            || record.contract != descriptor.contract.as_str()
            || record.model != model
            || !packet_ids.insert(record.packet_id.as_str())
            || !valid_sha256_tag(&record.output_hash)
            || record.output_len == 0
        {
            return Err(WorkError::InvalidPlan(format!(
                "packet evidence at ordinal {ordinal} does not match the expanded plan"
            )));
        }
        let output_path = record.output_path.canonicalize()?;
        let output = fs::read_to_string(&output_path)?;
        if !output_path.starts_with(&plan_dir)
            || fs::metadata(&output_path)?.len() != record.output_len as u64
            || sha256_file(&output_path)? != record.output_hash
        {
            return Err(WorkError::InvalidPlan(format!(
                "packet {} output evidence does not recompute",
                record.packet_id
            )));
        }
        let validated = validate_packet_output(descriptor, &output)?;
        if descriptor.contract == PacketContract::FileManifest {
            validate_files(&validated.files)?;
            manifest_files = validated.files;
        }
        ordered_packet_ids.push(record.packet_id.clone());
    }
    validate_packet_schedule(&descriptors, &manifest_files)?;
    let intent_hash = string(&manifest, "intent_hash")?;
    let fingerprint_hash = string(&manifest, "fingerprint_hash")?;
    if !valid_sha256_tag(intent_hash) || !valid_sha256_tag(fingerprint_hash) {
        return Err(WorkError::InvalidPlan(
            "work manifest identity hashes are invalid".to_string(),
        ));
    }
    let atom_stack = strings(&manifest, "atom_stack")?;
    let canonical = WorkPlan::canonical_from_manifest(
        intent_hash,
        string(&manifest, "recipe_id")?,
        &atom_stack,
        fingerprint_hash,
        manifest_files,
    )?;
    if canonical.id != expected_plan_id || canonical.packets.len() != descriptors.len() {
        return Err(WorkError::InvalidPlan(
            "work manifest identity or packet count is not canonical".to_string(),
        ));
    }
    for (actual, expected) in descriptors.iter().zip(&canonical.packets) {
        if actual.id != expected.id
            || actual.ordinal != expected.ordinal
            || actual.stage != expected.stage
            || actual.contract != expected.contract
            || actual.dependencies != expected.dependencies
            || actual.max_output_bytes != expected.max_output_bytes
            || actual.file.as_ref().map(|file| file.path.as_str())
                != expected.file.as_ref().map(|file| file.path.as_str())
        {
            return Err(WorkError::InvalidPlan(format!(
                "work packet {} does not match the canonical packet DAG",
                actual.id
            )));
        }
    }
    Ok(VerifiedWorkPlan {
        plan_id: expected_plan_id.to_string(),
        packet_count: expected_packet_count,
        model,
        packet_ids: ordered_packet_ids,
    })
}

fn validate_packet_schedule(
    descriptors: &[WorkPacket],
    files: &[crate::WorkFile],
) -> Result<(), WorkError> {
    let expected_count = 9 + files.len() * 4 + integration_group_count(files.len());
    if files.is_empty() || descriptors.len() != expected_count {
        return Err(WorkError::InvalidPlan(format!(
            "work packet schedule does not match manifest files: expected {expected_count}, got {}",
            descriptors.len()
        )));
    }
    let base = [
        (WorkStage::Intent, PacketContract::Envelope),
        (WorkStage::FunctionalRequirements, PacketContract::Envelope),
        (WorkStage::QualityRequirements, PacketContract::Envelope),
        (WorkStage::Architecture, PacketContract::Envelope),
        (WorkStage::FileManifest, PacketContract::FileManifest),
    ];
    for (index, (stage, contract)) in base.into_iter().enumerate() {
        expect_descriptor(&descriptors[index], index, stage, contract, "")?;
    }
    let file_stages = [
        (WorkStage::FileContract, PacketContract::Envelope),
        (WorkStage::FileImplementation, PacketContract::FileArtifact),
        (WorkStage::FileReview, PacketContract::Envelope),
        (WorkStage::FileCorrection, PacketContract::FileArtifact),
    ];
    let mut index = base.len();
    for file in files {
        for (stage, contract) in file_stages {
            expect_descriptor(&descriptors[index], index, stage, contract, &file.path)?;
            index += 1;
        }
    }
    for _ in 0..integration_group_count(files.len()) {
        expect_descriptor(
            &descriptors[index],
            index,
            WorkStage::IntegrationGroup,
            PacketContract::Envelope,
            "",
        )?;
        index += 1;
    }
    for (stage, contract) in [
        (WorkStage::Integration, PacketContract::Envelope),
        (WorkStage::Verification, PacketContract::Envelope),
        (WorkStage::AdversarialReview, PacketContract::Envelope),
        (WorkStage::Finalization, PacketContract::Envelope),
    ] {
        expect_descriptor(&descriptors[index], index, stage, contract, "")?;
        index += 1;
    }
    Ok(())
}

fn integration_group_count(mut inputs: usize) -> usize {
    let mut total = 0;
    while inputs > 3 {
        inputs = inputs.div_ceil(3);
        total += inputs;
    }
    total
}

fn expect_descriptor(
    packet: &WorkPacket,
    ordinal: usize,
    stage: WorkStage,
    contract: PacketContract,
    file_path: &str,
) -> Result<(), WorkError> {
    validate_id(&packet.id)?;
    let actual_path = packet
        .file
        .as_ref()
        .map(|file| file.path.as_str())
        .unwrap_or("");
    let prefix = format!("{:03}-{}-", ordinal + 1, stage.as_str());
    if packet.ordinal != ordinal
        || packet.stage != stage
        || packet.contract != contract
        || actual_path != file_path
        || !packet.id.starts_with(&prefix)
    {
        return Err(WorkError::InvalidPlan(format!(
            "work packet descriptor {ordinal} violates the meticulous schedule"
        )));
    }
    Ok(())
}

impl Default for WorkPlanStore {
    fn default() -> Self {
        Self::new(default_work_root())
    }
}

impl WorkPlanStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn acquire(&self, plan_id: &str) -> Result<WorkPlanLease, WorkError> {
        validate_id(plan_id)?;
        fs::create_dir_all(&self.root)?;
        let lock = self.root.join(format!("{plan_id}.lock"));
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(mut file) => {
                    let owner_token = format!(
                        "pid={} time_ms={} sequence={}",
                        std::process::id(),
                        now_ms(),
                        LEASE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
                    );
                    file.write_all(owner_token.as_bytes())?;
                    file.sync_all()?;
                    return Ok(WorkPlanLease {
                        path: lock,
                        owner_token,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock) {
                        match fs::remove_file(&lock) {
                            Ok(()) => continue,
                            Err(remove) if remove.kind() == std::io::ErrorKind::NotFound => {
                                continue
                            }
                            Err(_) => {}
                        }
                    }
                    if Instant::now() >= deadline {
                        return Err(WorkError::Io(format!(
                            "timed out acquiring plan lock {}",
                            lock.display()
                        )));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn write_plan_manifest(&self, plan: &WorkPlan) -> Result<PathBuf, WorkError> {
        plan.validate()?;
        let dir = self.plan_dir(&plan.id)?;
        fs::create_dir_all(&dir)?;
        let suffix = if plan.is_expanded() {
            "expanded"
        } else {
            "base"
        };
        let path = dir.join(format!("plan-{suffix}.json"));
        let atom_stack = plan
            .atom_stack
            .iter()
            .map(|item| format!("\"{}\"", json_escape(item)))
            .collect::<Vec<_>>()
            .join(",");
        let packets = plan
            .packets
            .iter()
            .map(|packet| {
                let dependencies = packet
                    .dependencies
                    .iter()
                    .map(|dependency| format!("\"{}\"", json_escape(dependency)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"id\":\"{}\",\"ordinal\":{},\"stage\":\"{}\",\"contract\":\"{}\",\"file_path\":\"{}\",\"dependencies\":[{}],\"max_output_bytes\":{}}}",
                    json_escape(&packet.id),
                    packet.ordinal,
                    packet.stage.as_str(),
                    packet.contract.as_str(),
                    json_escape(
                        packet
                            .file
                            .as_ref()
                            .map(|file| file.path.as_str())
                            .unwrap_or("")
                    ),
                    dependencies,
                    packet.max_output_bytes
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let text = format!(
            "{{\"schema_version\":{},\"plan_id\":\"{}\",\"intent_hash\":\"{}\",\"recipe_id\":\"{}\",\"atom_stack\":[{}],\"fingerprint_hash\":\"{}\",\"packet_count\":{},\"expanded\":{},\"packets\":[{}]}}",
            WORK_SCHEMA_VERSION,
            json_escape(&plan.id),
            sha256_tagged(plan.intent.as_bytes()),
            json_escape(&plan.recipe_id),
            atom_stack,
            sha256_tagged(plan.fingerprint.as_bytes()),
            plan.packets.len(),
            plan.is_expanded(),
            packets
        );
        write_immutable_verified(&path, text.as_bytes())?;
        Ok(path)
    }

    pub fn store_packet(
        &self,
        plan: &WorkPlan,
        packet: &WorkPacket,
        output: &str,
        model: &str,
    ) -> Result<StoredPacket, WorkError> {
        plan.validate()?;
        if !plan.packets.iter().any(|item| item == packet) {
            return Err(WorkError::InvalidPlan(format!(
                "packet {} does not belong to plan {}",
                packet.id, plan.id
            )));
        }
        let safe_output = validate_secure_packet_output(packet, output)?.context;
        let output_hash = sha256_tagged(safe_output.as_bytes());
        let hash_name = output_hash
            .strip_prefix("sha256:")
            .expect("tagged hash prefix");
        let dir = self.plan_dir(&plan.id)?;
        let output_dir = dir.join("outputs");
        fs::create_dir_all(&output_dir)?;
        let artifact = output_dir.join(format!("{hash_name}.txt"));
        write_immutable_verified(&artifact, safe_output.as_bytes())?;
        let record = StoredPacket {
            schema_version: WORK_SCHEMA_VERSION,
            plan_id: plan.id.clone(),
            packet_id: packet.id.clone(),
            ordinal: packet.ordinal,
            stage: packet.stage.as_str().to_string(),
            contract: packet.contract.as_str().to_string(),
            model: model.to_string(),
            output_path: artifact.clone(),
            output_hash,
            output_len: safe_output.len(),
        };
        let metadata = self.metadata_path(plan, packet)?;
        write_immutable_verified(&metadata, record.to_json().as_bytes())?;
        self.verify_record(plan, packet, &record, model)?;
        Ok(record)
    }

    pub fn load_packet(
        &self,
        plan: &WorkPlan,
        packet: &WorkPacket,
        model: &str,
    ) -> Result<Option<CompletedPacket>, WorkError> {
        let metadata = self.metadata_path(plan, packet)?;
        if !metadata.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&metadata)?;
        let record = StoredPacket::from_json(&text)?;
        self.verify_record(plan, packet, &record, model)?;
        let output = fs::read_to_string(&record.output_path)?;
        Ok(Some(CompletedPacket {
            packet_id: packet.id.clone(),
            output,
        }))
    }

    pub fn load_contiguous(
        &self,
        plan: &WorkPlan,
        model: &str,
    ) -> Result<Vec<CompletedPacket>, WorkError> {
        let mut completed = Vec::new();
        for packet in &plan.packets {
            match self.load_packet(plan, packet, model)? {
                Some(item) => completed.push(item),
                None => break,
            }
        }
        Ok(completed)
    }

    fn verify_record(
        &self,
        plan: &WorkPlan,
        packet: &WorkPacket,
        record: &StoredPacket,
        model: &str,
    ) -> Result<(), WorkError> {
        if record.schema_version != WORK_SCHEMA_VERSION
            || record.plan_id != plan.id
            || record.packet_id != packet.id
            || record.ordinal != packet.ordinal
            || record.stage != packet.stage.as_str()
            || record.contract != packet.contract.as_str()
            || record.model != model
            || !valid_sha256_tag(&record.output_hash)
            || record.output_len == 0
        {
            return Err(WorkError::InvalidPlan(format!(
                "stored packet {} metadata does not match its plan",
                packet.id
            )));
        }
        let plan_dir = self.plan_dir(&plan.id)?.canonicalize()?;
        let output_path = record.output_path.canonicalize()?;
        if !output_path.starts_with(&plan_dir) {
            return Err(WorkError::InvalidPlan(format!(
                "stored packet {} artifact escapes its plan directory",
                packet.id
            )));
        }
        let metadata = fs::metadata(&output_path)?;
        if metadata.len() != record.output_len as u64
            || sha256_file(&output_path)? != record.output_hash
        {
            return Err(WorkError::InvalidPlan(format!(
                "stored packet {} artifact evidence does not recompute",
                packet.id
            )));
        }
        Ok(())
    }

    fn plan_dir(&self, plan_id: &str) -> Result<PathBuf, WorkError> {
        validate_id(plan_id)?;
        Ok(self.root.join(plan_id))
    }

    fn metadata_path(&self, plan: &WorkPlan, packet: &WorkPacket) -> Result<PathBuf, WorkError> {
        validate_id(&plan.id)?;
        validate_id(&packet.id)?;
        Ok(self.plan_dir(&plan.id)?.join(format!(
            "{:03}-{}.json",
            packet.ordinal + 1,
            packet.stage.as_str()
        )))
    }
}

impl StoredPacket {
    fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":{},\"plan_id\":\"{}\",\"packet_id\":\"{}\",\"ordinal\":{},\"stage\":\"{}\",\"contract\":\"{}\",\"model\":\"{}\",\"output_path\":\"{}\",\"output_hash\":\"{}\",\"output_len\":{}}}",
            self.schema_version,
            json_escape(&self.plan_id),
            json_escape(&self.packet_id),
            self.ordinal,
            json_escape(&self.stage),
            json_escape(&self.contract),
            json_escape(&self.model),
            json_escape(&self.output_path.to_string_lossy()),
            json_escape(&self.output_hash),
            self.output_len
        )
    }

    fn from_json(input: &str) -> Result<Self, WorkError> {
        let value = parse_json(input)
            .map_err(|error| WorkError::InvalidPlan(format!("stored packet JSON: {error}")))?;
        let object = value
            .as_object()
            .ok_or_else(|| WorkError::InvalidPlan("stored packet is not an object".to_string()))?;
        let expected: HashSet<&str> = [
            "schema_version",
            "plan_id",
            "packet_id",
            "ordinal",
            "stage",
            "contract",
            "model",
            "output_path",
            "output_hash",
            "output_len",
        ]
        .into_iter()
        .collect();
        let actual: HashSet<&str> = object.iter().map(|(name, _)| name.as_str()).collect();
        if actual != expected {
            return Err(WorkError::InvalidPlan(
                "stored packet fields are not the exact schema".to_string(),
            ));
        }
        Ok(Self {
            schema_version: number(&value, "schema_version")? as u32,
            plan_id: string(&value, "plan_id")?.to_string(),
            packet_id: string(&value, "packet_id")?.to_string(),
            ordinal: number(&value, "ordinal")? as usize,
            stage: string(&value, "stage")?.to_string(),
            contract: string(&value, "contract")?.to_string(),
            model: string(&value, "model")?.to_string(),
            output_path: PathBuf::from(string(&value, "output_path")?),
            output_hash: string(&value, "output_hash")?.to_string(),
            output_len: number(&value, "output_len")? as usize,
        })
    }
}

fn write_immutable_verified(path: &Path, bytes: &[u8]) -> Result<(), WorkError> {
    if path.exists() {
        let existing = fs::read(path)?;
        if existing == bytes {
            return Ok(());
        }
        return Err(WorkError::Io(format!(
            "immutable work evidence conflict at {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| WorkError::Io("work evidence path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("packet"),
        std::process::id(),
        now_ms()
    ));
    let result = (|| -> Result<(), WorkError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let mut readback = Vec::new();
        File::open(&temp)?.read_to_end(&mut readback)?;
        if readback != bytes {
            return Err(WorkError::Io(format!(
                "work evidence readback mismatch at {}",
                temp.display()
            )));
        }
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn validate_id(value: &str) -> Result<(), WorkError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(WorkError::InvalidPlan(format!("unsafe work id: {value}")));
    }
    Ok(())
}

fn lock_is_stale(path: &Path) -> bool {
    let old_enough = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .map(|age| age > STALE_LOCK_AGE)
        .unwrap_or(false);
    if !old_enough {
        return false;
    }
    let pid = fs::read_to_string(path).ok().and_then(|text| {
        text.split_whitespace()
            .find_map(|part| part.strip_prefix("pid="))
            .and_then(|value| value.parse::<u32>().ok())
    });
    pid.map(|pid| !process_is_alive(pid)).unwrap_or(true)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            false
        } else {
            CloseHandle(handle);
            true
        }
    }
}

#[cfg(not(windows))]
fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn string<'a>(value: &'a JsonValue, key: &str) -> Result<&'a str, WorkError> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| WorkError::InvalidPlan(format!("stored packet {key} is not a string")))
}

fn number(value: &JsonValue, key: &str) -> Result<u64, WorkError> {
    value
        .get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| WorkError::InvalidPlan(format!("stored packet {key} is not an integer")))
}

fn strings(value: &JsonValue, key: &str) -> Result<Vec<String>, WorkError> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| WorkError::InvalidPlan(format!("stored packet {key} is not an array")))?
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                WorkError::InvalidPlan(format!("stored packet {key} entry is not a string"))
            })
        })
        .collect()
}

fn json_escape(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push(' '),
            ch => output.push(ch),
        }
    }
    output
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{validate_packet_output, WorkFile, WorkStage};

    fn temp_store(label: &str) -> (PathBuf, WorkPlanStore) {
        let path = std::env::temp_dir().join(format!(
            "math-atoms-work-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        (path.clone(), WorkPlanStore::new(path))
    }

    fn expanded_plan() -> WorkPlan {
        let mut plan =
            WorkPlan::meticulous("Build a calculator", "provider-model-loop", &[], "fixture")
                .unwrap();
        plan.expand_files(vec![WorkFile {
            path: "src/main.rs".into(),
            purpose: "entry".into(),
            acceptance: vec!["calculates".into()],
        }])
        .unwrap();
        plan
    }

    fn valid_output(packet: &WorkPacket) -> String {
        match packet.contract {
            PacketContract::Envelope => format!(
                "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"packet complete\",\"checks\":[\"verified\"],\"risks\":[]}}",
                packet.id
            ),
            PacketContract::FileManifest => format!(
                "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"files\":[{{\"path\":\"src/main.rs\",\"purpose\":\"entry\",\"acceptance\":[\"runs\"]}}],\"checks\":[\"covered\"],\"risks\":[]}}",
                packet.id
            ),
            PacketContract::FileArtifact => "```rust\nfn main() {}\n```".to_string(),
        }
    }

    #[test]
    fn packet_evidence_round_trips_and_recomputes() {
        let (path, store) = temp_store("roundtrip");
        let plan = expanded_plan();
        let packet = &plan.packets[0];
        let raw = format!(
            "{{\"packet_id\":\"{}\",\"status\":\"complete\",\"result\":\"normalized\",\"checks\":[\"preserved\"],\"risks\":[]}}",
            packet.id
        );
        let validated = validate_packet_output(packet, &raw).unwrap();
        let _lease = store.acquire(&plan.id).unwrap();
        store.write_plan_manifest(&plan).unwrap();
        let record = store
            .store_packet(&plan, packet, &validated.context, "fixture-model")
            .unwrap();
        assert_eq!(
            store
                .load_packet(&plan, packet, "fixture-model")
                .unwrap()
                .unwrap()
                .output,
            validated.context
        );
        assert_eq!(
            sha256_file(&record.output_path).unwrap(),
            record.output_hash
        );
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn expanded_plan_verifier_recomputes_every_packet() {
        let (path, store) = temp_store("verify-plan");
        let plan = expanded_plan();
        let _lease = store.acquire(&plan.id).unwrap();
        let manifest = store.write_plan_manifest(&plan).unwrap();
        for packet in &plan.packets {
            store
                .store_packet(&plan, packet, &valid_output(packet), "fixture-model")
                .unwrap();
        }
        let verified = verify_work_plan_evidence(&manifest, &plan.id, plan.packets.len()).unwrap();
        assert_eq!(verified.packet_count, 13);
        assert_eq!(verified.model, "fixture-model");
        let tampered = fs::read_to_string(&manifest).unwrap().replacen(
            "\"stage\":\"file-contract\"",
            "\"stage\":\"integration\"",
            1,
        );
        fs::write(&manifest, tampered).unwrap();
        assert!(verify_work_plan_evidence(&manifest, &plan.id, plan.packets.len()).is_err());
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn tampered_packet_artifact_fails_closed() {
        let (path, store) = temp_store("tamper");
        let plan = expanded_plan();
        let packet = &plan.packets[0];
        let _lease = store.acquire(&plan.id).unwrap();
        let record = store
            .store_packet(&plan, packet, &valid_output(packet), "model")
            .unwrap();
        fs::write(&record.output_path, "tampered").unwrap();
        assert!(store.load_packet(&plan, packet, "model").is_err());
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn model_mismatch_cannot_resume_packet() {
        let (path, store) = temp_store("model");
        let plan = expanded_plan();
        let packet = &plan.packets[0];
        let _lease = store.acquire(&plan.id).unwrap();
        store
            .store_packet(&plan, packet, &valid_output(packet), "model-a")
            .unwrap();
        assert!(store.load_packet(&plan, packet, "model-b").is_err());
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn stored_file_artifact_preserves_non_secret_code_byte_for_byte() {
        let (path, store) = temp_store("redact");
        let plan = expanded_plan();
        let packet = plan
            .packets
            .iter()
            .find(|packet| packet.stage == WorkStage::FileImplementation)
            .unwrap();
        let _lease = store.acquire(&plan.id).unwrap();
        let output = "```rust\nfn main() {\n    let key=42;\n    let token_count=3;\n    println!(\"{}\", key + token_count);\n}\n```";
        let record = store.store_packet(&plan, packet, output, "model").unwrap();
        let stored = fs::read_to_string(record.output_path).unwrap();
        assert_eq!(stored, output);
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn stored_file_artifact_rejects_embedded_credentials() {
        let (path, store) = temp_store("reject-secret");
        let plan = expanded_plan();
        let packet = plan
            .packets
            .iter()
            .find(|packet| packet.stage == WorkStage::FileImplementation)
            .unwrap();
        let _lease = store.acquire(&plan.id).unwrap();
        assert!(store
            .store_packet(
                &plan,
                packet,
                "```rust\nconst KEY: &str = \"sk-abcdefghijklmnopqrstuvwxyz\";\n```",
                "model"
            )
            .is_err());
        drop(_lease);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn active_process_is_not_treated_as_a_stale_plan_owner() {
        assert!(process_is_alive(std::process::id()));
    }

    #[test]
    fn lease_drop_never_removes_a_replacement_owner_lock() {
        let (path, store) = temp_store("lease-owner");
        let plan = expanded_plan();
        let lease = store.acquire(&plan.id).unwrap();
        let lock = path.join(format!("{}.lock", plan.id));
        fs::write(&lock, "pid=999999 time_ms=1 sequence=2").unwrap();
        drop(lease);
        assert!(lock.exists());
        fs::remove_file(lock).unwrap();
        fs::remove_dir_all(path).unwrap();
    }
}
