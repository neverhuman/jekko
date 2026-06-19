use crate::hero_judge::{HeroJudgeLaneArtifact, HeroJudgeSearchReceipt, PromotionDecision};
use crate::model_client::kind_label;
use crate::model_policy::ModelTaskKind;

use super::helpers::storage_safe_summary;

pub struct GenerationMetricInputs<'a> {
    pub run_id: &'a str,
    pub generation: usize,
    pub literature: &'a [HeroJudgeLaneArtifact],
    pub heroes: &'a [HeroJudgeLaneArtifact],
    pub judges: &'a [HeroJudgeLaneArtifact],
    pub verifiers: &'a [HeroJudgeLaneArtifact],
    pub red_team: &'a [HeroJudgeLaneArtifact],
    pub meta: &'a [HeroJudgeLaneArtifact],
    pub decision: &'a PromotionDecision,
    pub search_receipts: &'a [HeroJudgeSearchReceipt],
    pub previous_overall: Option<f64>,
    pub previous_frontier: Option<f64>,
    pub knowledge_entry_count: usize,
}

pub fn average_score(artifacts: &[HeroJudgeLaneArtifact], default_score: f64) -> f64 {
    if artifacts.is_empty() {
        return default_score;
    }
    artifacts.iter().map(|artifact| artifact.score).sum::<f64>() / artifacts.len() as f64
}

pub fn summary_from_value(
    kind: ModelTaskKind,
    generation: usize,
    lane: usize,
    value: &serde_json::Value,
) -> String {
    match value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .map(storage_safe_summary)
    {
        Some(summary) => summary,
        None => format!(
            "{} generation {generation} lane {lane} completed with storage-safe summary.",
            kind_label(kind)
        ),
    }
}

pub fn synthetic_lane_value(kind: ModelTaskKind, generation: usize) -> serde_json::Value {
    serde_json::json!({
        "summary": format!("deterministic {} summary", kind_label(kind)),
        "claims": ["bounded evidence", "canary checked", "promotion-gated"],
        "questions": ["What falsifiable signal would move this theory up or down?"],
        "rubric": ["evidence grounding", "falsifiability", "calibration"],
        "evidence_refs": ["deterministic-local-evidence"],
        "score": lane_default_score(kind, generation),
    })
}

pub fn parse_substitute_lane_value(kind: ModelTaskKind, generation: usize) -> serde_json::Value {
    serde_json::json!({
        "summary": format!("live {} response completed but required storage-safe JSON substitute", kind_label(kind)),
        "claims": [
            "live model call completed",
            "strict JSON parse failed",
            "raw provider text was not copied into the artifact"
        ],
        "questions": ["Which prompt constraint would make this lane return stricter structured JSON?"],
        "rubric": ["live receipt present", "storage-safe substitute", "requires reviewer caution"],
        "evidence_refs": ["live-model-receipt"],
        "score": (lane_default_score(kind, generation) * 0.75).clamp(0.0, 1.0),
        "parse_substitute": true,
    })
}

pub fn score_from_value(kind: ModelTaskKind, generation: usize, value: &serde_json::Value) -> f64 {
    match value.get("score").and_then(serde_json::Value::as_f64) {
        Some(score) => score.clamp(0.0, 1.0),
        None => lane_default_score(kind, generation),
    }
}

pub fn lane_default_score(kind: ModelTaskKind, generation: usize) -> f64 {
    let base = match kind {
        ModelTaskKind::HeroGenerate => 0.88,
        ModelTaskKind::JudgePatch => 0.82,
        ModelTaskKind::Verifier => 0.86,
        ModelTaskKind::LiteratureSynthesis => 0.80,
        ModelTaskKind::RedTeam => 0.20,
        ModelTaskKind::MetaJudge => 0.87,
        ModelTaskKind::KnowledgeCurate => 0.84,
        _ => 0.75,
    };
    (base + generation as f64 * 0.005).min(0.95)
}

pub fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
