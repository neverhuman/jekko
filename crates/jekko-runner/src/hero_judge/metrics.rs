use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Plot-ready lane row with fixed columns for CSV/JSONL aggregation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeLaneMetric {
    /// Run id.
    pub run_id: String,
    /// Generation.
    pub generation: usize,
    /// Coarse role bucket: `hero`, `judge`, `research`, or `knowledge`.
    pub role_group: String,
    /// Lane kind.
    pub kind: String,
    /// Artifact id.
    pub artifact_id: String,
    /// Lane index.
    pub lane: usize,
    /// Host score.
    pub score: f64,
    /// Claim quality.
    pub claim_quality: f64,
    /// Research/falsification-question quality.
    pub question_quality: f64,
    /// Rubric/judgment quality.
    pub rubric_quality: f64,
    /// Evidence grounding.
    pub evidence_grounding: f64,
    /// Structure/schema completeness.
    pub structural_completeness: f64,
    /// Storage safety.
    pub storage_safety: f64,
    /// Count-like host metrics.
    pub claim_count: f64,
    pub question_count: f64,
    pub rubric_item_count: f64,
    /// Receipt and artifact references.
    pub model_receipt_id: String,
    pub content_sha256: String,
    pub status: String,
}

/// Storage-safe card intended for independent reviewer packets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeReviewCard {
    /// Artifact id.
    pub artifact_id: String,
    /// Coarse role bucket.
    pub role_group: String,
    /// Lane kind.
    pub kind: String,
    /// Generation.
    pub generation: usize,
    /// Lane index.
    pub lane: usize,
    /// Host score.
    pub score: f64,
    /// Storage-safe summary only.
    pub summary: String,
    /// Content hash for audit.
    pub content_sha256: String,
    /// Plot-ready lane metrics.
    pub metrics: BTreeMap<String, f64>,
}

/// Per-generation plot-ready quality metrics for Hero/Judge evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeQualityMetric {
    /// Run id.
    pub run_id: String,
    /// Generation.
    pub generation: usize,
    /// Host-scored quality of the proposed universal-physics ideas.
    pub theory_quality_index: f64,
    /// Host-scored quality of generated research/falsification questions.
    pub question_quality_index: f64,
    /// Host-scored quality of judge/rubric patches.
    pub rubric_quality_index: f64,
    /// Agreement between judge scores and verifier scores.
    pub judge_calibration_index: f64,
    /// Evidence/search grounding strength.
    pub evidence_grounding_index: f64,
    /// Mean verifier confidence.
    pub verifier_confidence: f64,
    /// Resistance to red-team pressure.
    pub red_team_resilience: f64,
    /// Final deterministic promotion score.
    pub promotion_score: f64,
    /// Weighted quality score intended for trend plots.
    pub overall_quality_index: f64,
    /// Change in weighted score from the prior generation.
    pub delta_overall_quality: f64,
    /// Best retained quality score through this generation.
    pub frontier_quality_index: f64,
    /// Change in retained frontier quality from the prior generation.
    pub delta_frontier_quality: f64,
    /// Whether this generation promoted a candidate.
    pub promoted: bool,
    /// Candidate counts used for the metric.
    pub hero_candidate_count: usize,
    pub judge_patch_count: usize,
    pub research_receipt_count: usize,
    pub knowledge_entry_count: usize,
}

/// Summary of quality change over a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeQualityTrend {
    /// Run id.
    pub run_id: String,
    /// Completed generations.
    pub generations: usize,
    /// First overall quality score.
    pub start_overall_quality: f64,
    /// Latest overall quality score.
    pub latest_overall_quality: f64,
    /// Latest minus first score.
    pub delta_overall_quality: f64,
    /// First retained frontier quality score.
    pub start_frontier_quality: f64,
    /// Latest retained frontier quality score.
    pub latest_frontier_quality: f64,
    /// Latest minus first retained frontier score.
    pub delta_frontier_quality: f64,
    /// Best generation by overall score.
    pub best_generation: usize,
    /// Best overall score.
    pub best_overall_quality: f64,
    /// Whether the latest score improved over the first score.
    pub improved: bool,
    /// Column names useful for plotting.
    pub metric_keys: Vec<String>,
}
