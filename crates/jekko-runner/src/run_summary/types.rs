//! Schema types for the `zyal.run_summary.v1` artifact.
//!
//! Every ZYAL run emits one of these post-finalize. The struct is the
//! single thing a future agent reads to understand a run — see
//! `docs/ZYAL/AGENT_PLAYBOOK.md` for field-by-field interpretation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "zyal.run_summary.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSummary {
    pub schema_version: String,
    pub run_id: String,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub pipeline: String,
    pub manifest: Option<ManifestInfo>,
    pub terminal_status: String,
    pub halt_reason: Option<HaltReason>,
    pub pipeline_progress: PipelineProgress,
    pub model_calls: ModelCallStats,
    pub budget: BudgetSummary,
    pub balancer: BalancerSummary,
    pub signals_fired: Vec<SignalRow>,
    pub gates: BTreeMap<String, String>,
    pub artifact_paths: BTreeMap<String, String>,
    pub links: BTreeMap<String, String>,
    pub operator_next_steps: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub path: Option<String>,
}

/// What halted the pipeline, when a terminal_status is anything other than
/// `run_finished`. `None` for clean runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HaltReason {
    pub kind: String,
    pub stage: Option<String>,
    pub consecutive_attempts: Option<u32>,
    pub providers_tried: Vec<String>,
    pub users_tried: Vec<String>,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PipelineProgress {
    pub stages_reached: Vec<String>,
    pub stages_completed: Vec<String>,
    pub deepest_stage: Option<String>,
    pub artifacts_produced: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelCallStats {
    pub total_attempts: u64,
    pub parsed_outcomes: u64,
    pub retryable_failures: u64,
    pub final_blocks: u64,
    pub empty_responses: u64,
    pub by_user: BTreeMap<String, u64>,
    pub by_provider: BTreeMap<String, u64>,
    pub by_kind: BTreeMap<String, u64>,
    pub by_state: BTreeMap<String, u64>,
    pub by_quality_band: BTreeMap<String, u64>,
    pub latency_p50_ms: Option<u64>,
    pub latency_p95_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BudgetSummary {
    pub max_calls: Option<u64>,
    pub used: u64,
    pub remaining: Option<u64>,
    pub exhausted: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BalancerSummary {
    pub before_cursor: Option<i64>,
    pub after_cursor: Option<i64>,
    pub delta: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalRow {
    pub id: String,
    pub name: String,
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<serde_json::Value>,
}

impl RunSummary {
    /// Construct an empty summary keyed by run_id. Defaults to
    /// `terminal_status = "halted"` until the build pass observes a
    /// `RunFinished` event.
    pub fn empty(run_id: &str) -> Self {
        let mut links = BTreeMap::new();
        links.insert("spec".to_string(), "docs/ZYAL/SPEC.md".to_string());
        links.insert(
            "observability".to_string(),
            "docs/ZYAL/OBSERVABILITY.md".to_string(),
        );
        links.insert(
            "playbook".to_string(),
            "docs/ZYAL/AGENT_PLAYBOOK.md".to_string(),
        );
        links.insert(
            "quality_band".to_string(),
            "docs/ZYAL/MODEL_QUALITY_BAND.md".to_string(),
        );
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            run_id: run_id.to_string(),
            started_at: None,
            finished_at: None,
            duration_seconds: None,
            pipeline: "unknown".to_string(),
            manifest: None,
            terminal_status: "halted".to_string(),
            halt_reason: None,
            pipeline_progress: PipelineProgress::default(),
            model_calls: ModelCallStats::default(),
            budget: BudgetSummary::default(),
            balancer: BalancerSummary::default(),
            signals_fired: Vec::new(),
            gates: BTreeMap::new(),
            artifact_paths: BTreeMap::new(),
            links,
            operator_next_steps: Vec::new(),
        }
    }

    /// Compute artifact_paths for the standard set of files that live
    /// alongside the run directory. Sets only the paths that exist on disk
    /// so a consumer can `if Some(p)` cleanly.
    pub fn populate_artifact_paths(&mut self, run_dir: &std::path::Path) {
        let candidates: &[(&str, &str)] = &[
            ("events_jsonl", "events.jsonl"),
            ("model_receipts_jsonl", "model_receipts.jsonl"),
            ("replay_receipt_json", "replay_receipt.json"),
            ("reviewer_packet_json", "reviewer_packet.json"),
            ("superreasoning_packet_json", "superreasoning_packet.json"),
            ("claim_ledger_jsonl", "claim_ledger.jsonl"),
            ("unsupported_claims_jsonl", "unsupported_claims.jsonl"),
            ("negative_memory_jsonl", "negative_memory.jsonl"),
            ("headless_state_json", "STATE.json"),
            ("headless_state_md", "STATE.md"),
        ];
        for (key, name) in candidates {
            let p: PathBuf = run_dir.join(name);
            if p.exists() {
                self.artifact_paths
                    .insert((*key).to_string(), p.display().to_string());
            }
        }
    }
}
