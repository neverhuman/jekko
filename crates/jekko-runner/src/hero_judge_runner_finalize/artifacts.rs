use anyhow::Result;

use crate::hero_judge::{
    FrontierScore, HeroJudgeLaneMetric, HeroJudgeQualityMetric, KnowledgeEntry, PromotionDecision,
    PromptVariant,
};
use crate::hero_judge_eval::{
    quality_trend, write_json_pretty, write_jsonl, write_lane_metrics_csv, write_quality_csv,
};
use crate::hero_judge_runner_helpers::filter_lane_metrics;

use super::paths::RunArtifactPaths;

#[allow(clippy::too_many_arguments)]
pub(super) fn write_core_artifacts(
    paths: &RunArtifactPaths,
    run_id: &str,
    prompt_lineage: &[PromptVariant],
    scoreboard: &[FrontierScore],
    last_decision: &PromotionDecision,
    knowledge: &[KnowledgeEntry],
    quality_metrics: &[HeroJudgeQualityMetric],
    lane_metrics: &[HeroJudgeLaneMetric],
) -> Result<()> {
    write_json_pretty(&paths.prompt_lineage_json, &prompt_lineage.to_vec())?;
    write_json_pretty(&paths.frontier_scoreboard_json, &scoreboard.to_vec())?;
    write_json_pretty(&paths.promotion_decision_json, last_decision)?;
    write_jsonl(&paths.knowledge_compound_jsonl, knowledge)?;
    write_jsonl(&paths.quality_metrics_jsonl, quality_metrics)?;
    write_quality_csv(&paths.quality_metrics_csv, quality_metrics)?;
    write_jsonl(&paths.lane_metrics_jsonl, lane_metrics)?;
    write_lane_metrics_csv(&paths.lane_metrics_csv, lane_metrics)?;
    write_lane_metrics_csv(
        &paths.hero_metrics_csv,
        &filter_lane_metrics(lane_metrics, "hero"),
    )?;
    write_lane_metrics_csv(
        &paths.judge_metrics_csv,
        &filter_lane_metrics(lane_metrics, "judge"),
    )?;
    write_json_pretty(
        &paths.quality_trend_json,
        &quality_trend(run_id, quality_metrics),
    )?;
    Ok(())
}
