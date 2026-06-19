//! Public summary and tick-report types for the advanced reasoning runner.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model_client::ModelCallReceipt;
use crate::port::PortMasterPlan;

/// Advanced reasoning tick summary returned to CLI/server callers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvancedReasoningSummary {
    /// Last state reached.
    pub state: String,
    /// Artifact count.
    pub artifact_count: usize,
    /// Lane count.
    pub lane_count: usize,
    /// Memory capsule count.
    pub memory_capsule_count: usize,
    /// Parity gap count.
    pub parity_gap_count: usize,
    /// Reasoning graph export.
    pub reasoning_graph_json: PathBuf,
    /// Parity raw JSONL.
    pub parity_raw_jsonl: PathBuf,
    /// Parity summary JSON.
    pub parity_summary_json: PathBuf,
    /// Parity gaps JSON.
    pub parity_gaps_json: PathBuf,
    /// Generated parity manifest JSON.
    pub parity_generated_manifest_json: PathBuf,
    /// Approved CI case id list.
    pub parity_approved_ci_txt: PathBuf,
    /// Stage-0 proof artifact, when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage0_master_plan_json: Option<PathBuf>,
    /// Reasoning benchmark artifact, when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_benchmark_json: Option<PathBuf>,
}

/// Advanced tick output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvancedReasoningTickReport {
    /// Run id.
    pub run_id: String,
    /// Target id.
    pub target_id: String,
    /// Finalized plan.
    pub plan: PortMasterPlan,
    /// Last model receipt.
    pub model_receipt: ModelCallReceipt,
    /// Graph summary by kind.
    pub graph_summary: serde_json::Value,
    /// Fake task completed, if enabled.
    pub fake_task_completed: Option<String>,
    /// Advanced summary.
    pub advanced: AdvancedReasoningSummary,
}
