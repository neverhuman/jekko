//! Durable advanced-reasoning tables for ZYAL daemon runs.

mod artifacts;
mod memory;
mod reliability;
mod rows;

pub use artifacts::{
    list_reasoning_artifacts_for_run, list_reasoning_edges_for_run, list_reasoning_lanes_for_run,
    upsert_reasoning_artifact, upsert_reasoning_edge, upsert_reasoning_lane,
};
pub use memory::{
    cosine_similarity, decode_embedding, encode_embedding, list_memory_capsules_for_run,
    list_promoted_capsules, upsert_memory_capsule,
};
pub use reliability::{
    get_model_reliability, list_model_reliability, record_model_reliability_outcome,
    upsert_model_reliability,
};
pub use rows::{
    MemoryCapsuleRow, ModelReliabilityRow, ReasoningArtifactRow, ReasoningEdgeRow, ReasoningLaneRow,
};
