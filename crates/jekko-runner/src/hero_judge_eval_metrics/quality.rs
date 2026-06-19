use crate::hero_judge::{HeroJudgeQualityMetric, HeroJudgeQualityTrend};

use super::defaults::{average_score, rounded, GenerationMetricInputs};
use super::helpers::{
    max_metric, mean_metric, mean_metric_all, red_team_penalty, search_receipt_score,
};

pub fn generation_quality_metric(input: GenerationMetricInputs<'_>) -> HeroJudgeQualityMetric {
    let literature_support = average_score(input.literature, 0.80);
    let verifier_confidence = average_score(input.verifiers, 0.84);
    let judge_score = average_score(input.judges, verifier_confidence);
    let meta_score = average_score(input.meta, verifier_confidence);
    let red_team_resilience = (1.0 - (red_team_penalty(input.red_team) / 0.08)).clamp(0.0, 1.0);
    let theory_quality = (input.decision.score * 0.65
        + verifier_confidence * 0.15
        + literature_support * 0.10
        + red_team_resilience * 0.10)
        .clamp(0.0, 1.0);
    let question_quality = (mean_metric(input.heroes, "question_quality", 0.50) * 0.70
        + max_metric(input.heroes, "question_quality", 0.50) * 0.30)
        .clamp(0.0, 1.0);
    let rubric_quality = (mean_metric(input.judges, "rubric_quality", 0.50) * 0.65
        + mean_metric(input.verifiers, "rubric_quality", 0.50) * 0.20
        + mean_metric(input.meta, "rubric_quality", 0.50) * 0.15)
        .clamp(0.0, 1.0);
    let judge_calibration = (1.0 - (judge_score - verifier_confidence).abs() * 2.0)
        .min((1.0 - (meta_score - verifier_confidence).abs() * 2.0).clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
    let lane_grounding = mean_metric_all(
        &[
            input.literature,
            input.heroes,
            input.judges,
            input.verifiers,
            input.red_team,
            input.meta,
        ],
        "evidence_grounding",
        0.50,
    );
    let search_grounding = search_receipt_score(input.search_receipts);
    let evidence_grounding = (lane_grounding * 0.70 + search_grounding * 0.30).clamp(0.0, 1.0);
    let overall_quality = (theory_quality * 0.35
        + question_quality * 0.15
        + rubric_quality * 0.20
        + judge_calibration * 0.10
        + evidence_grounding * 0.10
        + red_team_resilience * 0.10)
        .clamp(0.0, 1.0);
    let delta = input
        .previous_overall
        .map(|previous| overall_quality - previous)
        .unwrap_or(0.0);
    let previous_frontier = input.previous_frontier.unwrap_or(overall_quality);
    let frontier_quality = previous_frontier.max(overall_quality);
    let delta_frontier = frontier_quality - previous_frontier;

    HeroJudgeQualityMetric {
        run_id: input.run_id.to_string(),
        generation: input.generation,
        theory_quality_index: rounded(theory_quality),
        question_quality_index: rounded(question_quality),
        rubric_quality_index: rounded(rubric_quality),
        judge_calibration_index: rounded(judge_calibration),
        evidence_grounding_index: rounded(evidence_grounding),
        verifier_confidence: rounded(verifier_confidence),
        red_team_resilience: rounded(red_team_resilience),
        promotion_score: rounded(input.decision.score),
        overall_quality_index: rounded(overall_quality),
        delta_overall_quality: rounded(delta),
        frontier_quality_index: rounded(frontier_quality),
        delta_frontier_quality: rounded(delta_frontier),
        promoted: input.decision.promoted,
        hero_candidate_count: input.heroes.len(),
        judge_patch_count: input.judges.len(),
        research_receipt_count: input.search_receipts.len(),
        knowledge_entry_count: input.knowledge_entry_count,
    }
}

pub fn quality_trend(run_id: &str, metrics: &[HeroJudgeQualityMetric]) -> HeroJudgeQualityTrend {
    let first = metrics.first();
    let latest = metrics.last();
    let best = metrics
        .iter()
        .max_by(|a, b| a.overall_quality_index.total_cmp(&b.overall_quality_index));
    let start = first
        .map(|metric| metric.overall_quality_index)
        .unwrap_or(0.0);
    let latest_value = latest
        .map(|metric| metric.overall_quality_index)
        .unwrap_or(0.0);
    let start_frontier = first
        .map(|metric| metric.frontier_quality_index)
        .unwrap_or(0.0);
    let latest_frontier = latest
        .map(|metric| metric.frontier_quality_index)
        .unwrap_or(0.0);
    HeroJudgeQualityTrend {
        run_id: run_id.to_string(),
        generations: metrics.len(),
        start_overall_quality: start,
        latest_overall_quality: latest_value,
        delta_overall_quality: rounded(latest_value - start),
        start_frontier_quality: start_frontier,
        latest_frontier_quality: latest_frontier,
        delta_frontier_quality: rounded(latest_frontier - start_frontier),
        best_generation: best.map(|metric| metric.generation).unwrap_or(0),
        best_overall_quality: best
            .map(|metric| metric.overall_quality_index)
            .unwrap_or(0.0),
        improved: latest_frontier > start_frontier,
        metric_keys: vec![
            "theory_quality_index".to_string(),
            "question_quality_index".to_string(),
            "rubric_quality_index".to_string(),
            "judge_calibration_index".to_string(),
            "evidence_grounding_index".to_string(),
            "red_team_resilience".to_string(),
            "overall_quality_index".to_string(),
            "frontier_quality_index".to_string(),
        ],
    }
}
