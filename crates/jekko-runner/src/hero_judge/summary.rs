use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One plot-ready row for a multi-run Hero/Judge series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeSeriesRow {
    /// Parent series id.
    pub series_id: String,
    /// 1-based trial index within the series.
    pub trial_index: usize,
    /// Child run id.
    pub run_id: String,
    /// Final generation represented by this row.
    pub generation: usize,
    /// Final quality metrics from the run.
    pub theory_quality_index: f64,
    pub question_quality_index: f64,
    pub rubric_quality_index: f64,
    pub judge_calibration_index: f64,
    pub evidence_grounding_index: f64,
    pub verifier_confidence: f64,
    pub red_team_resilience: f64,
    pub promotion_score: f64,
    pub overall_quality_index: f64,
    pub delta_overall_quality: f64,
    pub frontier_quality_index: f64,
    pub delta_frontier_quality: f64,
    /// Promotion result.
    pub promoted: bool,
    /// Winning candidate id when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontier_winner: Option<String>,
    /// Model/search accounting.
    pub model_calls_used: usize,
    pub model_call_budget: usize,
    pub search_receipt_count: usize,
    /// Final-generation lane means.
    pub hero_lane_mean: f64,
    pub judge_lane_mean: f64,
    /// Hashes for reviewer-traceable artifacts.
    pub quality_metrics_sha256: String,
    pub lane_metrics_sha256: String,
    pub reviewer_packet_sha256: String,
    pub promotion_decision_sha256: String,
    pub search_receipts_sha256: String,
}

/// Scoreboard entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierScore {
    /// Candidate id.
    pub candidate_id: String,
    /// Prompt id.
    pub prompt_id: String,
    /// Generation.
    pub generation: usize,
    /// Score.
    pub score: f64,
    /// Verifier score.
    pub verifier_score: f64,
    /// Red-team penalty.
    pub red_team_penalty: f64,
    /// Leak/canary status.
    pub leak_status: String,
    /// Promotion status.
    pub status: String,
}

/// Promotion decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionDecision {
    /// Run id.
    pub run_id: String,
    /// Generation.
    pub generation: usize,
    /// Winner candidate id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_candidate_id: Option<String>,
    /// Winner prompt id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_prompt_id: Option<String>,
    /// Winner score.
    pub score: f64,
    /// Whether the variant was promoted.
    pub promoted: bool,
    /// Decision reason.
    pub reason: String,
}

/// Knowledge-compound ledger row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    /// Entry id.
    pub id: String,
    /// Generation.
    pub generation: usize,
    /// `verified` or `rejected`.
    pub status: String,
    /// Claim summary.
    pub claim: String,
    /// Evidence references.
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    /// Source candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_candidate_id: Option<String>,
    /// Content hash.
    pub content_sha256: String,
}

/// Storage-safe deterministic search receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroJudgeSearchReceipt {
    /// Receipt id.
    pub id: String,
    /// Provider id.
    pub provider: String,
    /// Query.
    pub query: String,
    /// Status.
    pub status: String,
    /// Optional reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Number of links or hits.
    pub url_count: usize,
    /// Content hash.
    pub content_sha256: String,
}

/// Runner summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeRunSummary {
    /// Run id.
    pub run_id: String,
    /// Output directory.
    pub output_dir: PathBuf,
    /// Generations completed.
    pub generation: usize,
    /// Hero lane count.
    pub hero_lane_count: usize,
    /// Judge lane count.
    pub judge_lane_count: usize,
    /// Frontier winner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontier_winner: Option<String>,
    /// Knowledge entries.
    pub knowledge_entry_count: usize,
    /// Search receipts.
    pub search_receipt_count: usize,
    /// Last promotion decision.
    pub last_promotion_decision: PromotionDecision,
    /// Model calls used.
    pub model_calls_used: usize,
    /// Model call budget.
    pub model_call_budget: usize,
    /// Last model task kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model_kind: Option<String>,
    /// Artifact paths.
    pub prompt_lineage_json: PathBuf,
    pub frontier_scoreboard_json: PathBuf,
    pub promotion_decision_json: PathBuf,
    pub knowledge_compound_jsonl: PathBuf,
    pub search_receipts_json: PathBuf,
    pub quality_metrics_jsonl: PathBuf,
    pub quality_metrics_csv: PathBuf,
    pub quality_trend_json: PathBuf,
    pub lane_metrics_jsonl: PathBuf,
    pub lane_metrics_csv: PathBuf,
    pub hero_metrics_csv: PathBuf,
    pub judge_metrics_csv: PathBuf,
    pub reviewer_packet_json: PathBuf,
    pub superreasoning_packet_json: PathBuf,
    pub superreasoning_packet_sha256: String,
    pub replay_receipt_json: PathBuf,
    pub model_receipts_jsonl: PathBuf,
    pub claim_ledger_jsonl: PathBuf,
    pub unsupported_claims_jsonl: PathBuf,
    pub negative_memory_jsonl: PathBuf,
    pub headless_state_json: PathBuf,
    pub headless_state_md: PathBuf,
    pub complete_ok: PathBuf,
}

/// Summary for a multi-run Hero/Judge series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeSeriesSummary {
    /// Series id.
    pub series_id: String,
    /// Output directory for aggregate artifacts.
    pub output_dir: PathBuf,
    /// Number of completed runs.
    pub run_count: usize,
    /// Child run summaries.
    pub runs: Vec<HeroJudgeRunSummary>,
    /// Aggregate artifacts.
    pub run_summaries_jsonl: PathBuf,
    pub quality_metrics_jsonl: PathBuf,
    pub quality_metrics_csv: PathBuf,
    pub lane_metrics_jsonl: PathBuf,
    pub lane_metrics_csv: PathBuf,
    pub hero_metrics_csv: PathBuf,
    pub judge_metrics_csv: PathBuf,
    pub series_summary_csv: PathBuf,
    pub reviewer_index_json: PathBuf,
    pub complete_ok: PathBuf,
}
