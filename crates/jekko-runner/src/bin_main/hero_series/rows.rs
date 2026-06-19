use anyhow::{Context, Result};

use jekko_runner::hero_judge::{
    HeroJudgeLaneMetric, HeroJudgeQualityMetric, HeroJudgeRunSummary, HeroJudgeSeriesRow,
};

use super::files::file_sha256;

pub(super) fn series_rows(
    series_id: &str,
    runs: &[HeroJudgeRunSummary],
    quality_metrics: &[HeroJudgeQualityMetric],
    lane_metrics: &[HeroJudgeLaneMetric],
) -> Result<Vec<HeroJudgeSeriesRow>> {
    runs.iter()
        .enumerate()
        .map(|(index, run)| {
            let final_metric = quality_metrics
                .iter()
                .filter(|metric| metric.run_id == run.run_id)
                .max_by_key(|metric| metric.generation)
                .with_context(|| format!("missing quality metrics for {}", run.run_id))?;
            Ok(HeroJudgeSeriesRow {
                series_id: series_id.to_string(),
                trial_index: index + 1,
                run_id: run.run_id.clone(),
                generation: final_metric.generation,
                theory_quality_index: final_metric.theory_quality_index,
                question_quality_index: final_metric.question_quality_index,
                rubric_quality_index: final_metric.rubric_quality_index,
                judge_calibration_index: final_metric.judge_calibration_index,
                evidence_grounding_index: final_metric.evidence_grounding_index,
                verifier_confidence: final_metric.verifier_confidence,
                red_team_resilience: final_metric.red_team_resilience,
                promotion_score: final_metric.promotion_score,
                overall_quality_index: final_metric.overall_quality_index,
                delta_overall_quality: final_metric.delta_overall_quality,
                frontier_quality_index: final_metric.frontier_quality_index,
                delta_frontier_quality: final_metric.delta_frontier_quality,
                promoted: final_metric.promoted,
                frontier_winner: run.frontier_winner.clone(),
                model_calls_used: run.model_calls_used,
                model_call_budget: run.model_call_budget,
                search_receipt_count: run.search_receipt_count,
                hero_lane_mean: rounded(mean_lane_score(
                    &run.run_id,
                    final_metric.generation,
                    lane_metrics,
                    "hero",
                )),
                judge_lane_mean: rounded(mean_lane_score(
                    &run.run_id,
                    final_metric.generation,
                    lane_metrics,
                    "judge",
                )),
                quality_metrics_sha256: file_sha256(&run.quality_metrics_jsonl)?,
                lane_metrics_sha256: file_sha256(&run.lane_metrics_jsonl)?,
                reviewer_packet_sha256: file_sha256(&run.reviewer_packet_json)?,
                promotion_decision_sha256: file_sha256(&run.promotion_decision_json)?,
                search_receipts_sha256: file_sha256(&run.search_receipts_json)?,
            })
        })
        .collect()
}

fn mean_lane_score(
    run_id: &str,
    generation: usize,
    metrics: &[HeroJudgeLaneMetric],
    role_group: &str,
) -> f64 {
    let mut total = 0.0;
    let mut count = 0_usize;
    for metric in metrics {
        if metric.run_id == run_id
            && metric.generation == generation
            && metric.role_group == role_group
        {
            total += metric.score;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
