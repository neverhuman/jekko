use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::hero_judge::{HeroJudgeLaneMetric, HeroJudgeQualityMetric, HeroJudgeSeriesRow};

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn write_json_pretty<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn write_jsonl<T: serde::Serialize>(path: &Path, values: &[T]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    for value in values {
        writeln!(file, "{}", serde_json::to_string(value)?)?;
    }
    Ok(())
}

pub fn write_quality_csv(path: &Path, metrics: &[HeroJudgeQualityMetric]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(
        file,
        "run_id,generation,theory_quality_index,question_quality_index,rubric_quality_index,judge_calibration_index,evidence_grounding_index,verifier_confidence,red_team_resilience,promotion_score,overall_quality_index,delta_overall_quality,frontier_quality_index,delta_frontier_quality,promoted,hero_candidate_count,judge_patch_count,research_receipt_count,knowledge_entry_count"
    )?;
    for metric in metrics {
        writeln!(
            file,
            "{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{}",
            metric.run_id,
            metric.generation,
            metric.theory_quality_index,
            metric.question_quality_index,
            metric.rubric_quality_index,
            metric.judge_calibration_index,
            metric.evidence_grounding_index,
            metric.verifier_confidence,
            metric.red_team_resilience,
            metric.promotion_score,
            metric.overall_quality_index,
            metric.delta_overall_quality,
            metric.frontier_quality_index,
            metric.delta_frontier_quality,
            metric.promoted,
            metric.hero_candidate_count,
            metric.judge_patch_count,
            metric.research_receipt_count,
            metric.knowledge_entry_count,
        )?;
    }
    Ok(())
}

pub fn write_lane_metrics_csv(path: &Path, metrics: &[HeroJudgeLaneMetric]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(
        file,
        "run_id,generation,role_group,kind,artifact_id,lane,score,claim_quality,question_quality,rubric_quality,evidence_grounding,structural_completeness,storage_safety,claim_count,question_count,rubric_item_count,status,model_receipt_id,content_sha256"
    )?;
    for metric in metrics {
        writeln!(
            file,
            "{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.0},{:.0},{:.0},{},{},{}",
            metric.run_id,
            metric.generation,
            metric.role_group,
            metric.kind,
            metric.artifact_id,
            metric.lane,
            metric.score,
            metric.claim_quality,
            metric.question_quality,
            metric.rubric_quality,
            metric.evidence_grounding,
            metric.structural_completeness,
            metric.storage_safety,
            metric.claim_count,
            metric.question_count,
            metric.rubric_item_count,
            metric.status,
            metric.model_receipt_id,
            metric.content_sha256,
        )?;
    }
    Ok(())
}

pub fn write_series_summary_csv(path: &Path, rows: &[HeroJudgeSeriesRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(
        file,
        "series_id,trial_index,run_id,generation,theory_quality_index,question_quality_index,rubric_quality_index,judge_calibration_index,evidence_grounding_index,verifier_confidence,red_team_resilience,promotion_score,overall_quality_index,delta_overall_quality,frontier_quality_index,delta_frontier_quality,promoted,frontier_winner,model_calls_used,model_call_budget,search_receipt_count,hero_lane_mean,judge_lane_mean,quality_metrics_sha256,lane_metrics_sha256,reviewer_packet_sha256,promotion_decision_sha256,search_receipts_sha256"
    )?;
    for row in rows {
        writeln!(
            file,
            "{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{},{:.3},{:.3},{},{},{},{},{}",
            csv_cell(&row.series_id),
            row.trial_index,
            csv_cell(&row.run_id),
            row.generation,
            row.theory_quality_index,
            row.question_quality_index,
            row.rubric_quality_index,
            row.judge_calibration_index,
            row.evidence_grounding_index,
            row.verifier_confidence,
            row.red_team_resilience,
            row.promotion_score,
            row.overall_quality_index,
            row.delta_overall_quality,
            row.frontier_quality_index,
            row.delta_frontier_quality,
            row.promoted,
            csv_cell(row.frontier_winner.as_deref().unwrap_or("")),
            row.model_calls_used,
            row.model_call_budget,
            row.search_receipt_count,
            row.hero_lane_mean,
            row.judge_lane_mean,
            csv_cell(&row.quality_metrics_sha256),
            csv_cell(&row.lane_metrics_sha256),
            csv_cell(&row.reviewer_packet_sha256),
            csv_cell(&row.promotion_decision_sha256),
            csv_cell(&row.search_receipts_sha256),
        )?;
    }
    Ok(())
}
