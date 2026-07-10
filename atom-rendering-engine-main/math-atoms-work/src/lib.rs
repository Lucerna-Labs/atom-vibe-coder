//! Fine-grained provider work plans with strict contracts and durable evidence.

mod model;
mod planner;
mod store;

pub use model::{
    CompletedPacket, GeneratedFile, PacketContract, ValidatedPacketOutput, WorkError, WorkFile,
    WorkPacket, WorkPlan, WorkPrompt, WorkStage, MAX_FILES_PER_PLAN, MAX_INTENT_BYTES,
    WORK_SCHEMA_VERSION,
};
pub use planner::{
    extract_json_payload, validate_packet_output, validate_secure_file_artifact,
    validate_secure_packet_output,
};
pub use store::{
    default_work_root, verify_work_plan_evidence, StoredPacket, VerifiedWorkPlan, WorkPlanLease,
    WorkPlanStore,
};
