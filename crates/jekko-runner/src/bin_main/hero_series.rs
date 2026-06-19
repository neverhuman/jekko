use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use jekko_runner::hero_judge::{
    HeroJudgeLaneMetric, HeroJudgeQualityMetric, HeroJudgeRunbook, HeroJudgeSeriesSummary,
};
use jekko_runner::hero_judge_eval::{
    write_jsonl, write_lane_metrics_csv, write_quality_csv, write_series_summary_csv,
};

use super::cli::HeroJudgeRunArgs;

mod client;
mod files;
mod rows;
mod trials;

pub(crate) use client::hero_judge_client;

pub(crate) async fn run_hero_judge_series(
    repo: &Path,
    series_id: &str,
    args: &HeroJudgeRunArgs,
    runbook: HeroJudgeRunbook,
) -> Result<HeroJudgeSeriesSummary> {
    let series_dir = repo
        .join(runbook.hero_judge.output_root())
        .join(format!("{series_id}-series"));
    fs::create_dir_all(&series_dir).with_context(|| format!("mkdir {}", series_dir.display()))?;
    let runs = trials::run_series_trials(repo, series_id, args, runbook.clone()).await?;

    let mut quality_metrics = Vec::new();
    let mut lane_metrics = Vec::new();
    for summary in &runs {
        quality_metrics.extend(files::read_jsonl::<HeroJudgeQualityMetric>(
            &summary.quality_metrics_jsonl,
        )?);
        lane_metrics.extend(files::read_jsonl::<HeroJudgeLaneMetric>(
            &summary.lane_metrics_jsonl,
        )?);
    }

    let run_summaries_jsonl = series_dir.join("run_summaries.jsonl");
    let quality_metrics_jsonl = series_dir.join("quality_metrics.jsonl");
    let quality_metrics_csv = series_dir.join("quality_metrics.csv");
    let lane_metrics_jsonl = series_dir.join("lane_metrics.jsonl");
    let lane_metrics_csv = series_dir.join("lane_metrics.csv");
    let hero_metrics_csv = series_dir.join("hero_metrics.csv");
    let judge_metrics_csv = series_dir.join("judge_metrics.csv");
    let series_summary_csv = series_dir.join("series_summary.csv");
    let reviewer_index_json = series_dir.join("reviewer_index.json");
    let complete_ok = series_dir.join("complete.ok");
    let series_rows = rows::series_rows(series_id, &runs, &quality_metrics, &lane_metrics)?;

    write_jsonl(&run_summaries_jsonl, &runs)?;
    write_jsonl(&quality_metrics_jsonl, &quality_metrics)?;
    write_quality_csv(&quality_metrics_csv, &quality_metrics)?;
    write_jsonl(&lane_metrics_jsonl, &lane_metrics)?;
    write_lane_metrics_csv(&lane_metrics_csv, &lane_metrics)?;
    write_lane_metrics_csv(
        &hero_metrics_csv,
        &files::filter_series_lanes(&lane_metrics, "hero"),
    )?;
    write_lane_metrics_csv(
        &judge_metrics_csv,
        &files::filter_series_lanes(&lane_metrics, "judge"),
    )?;
    write_series_summary_csv(&series_summary_csv, &series_rows)?;
    fs::write(
        &reviewer_index_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "series_id": series_id,
            "run_count": runs.len(),
            "reviewer_packet_paths": runs
                .iter()
                .map(|run| run.reviewer_packet_json.display().to_string())
                .collect::<Vec<_>>(),
            "plot_files": {
                "quality_metrics_csv": quality_metrics_csv.display().to_string(),
                "lane_metrics_csv": lane_metrics_csv.display().to_string(),
                "hero_metrics_csv": hero_metrics_csv.display().to_string(),
                "judge_metrics_csv": judge_metrics_csv.display().to_string(),
                "series_summary_csv": series_summary_csv.display().to_string(),
            },
        }))?,
    )
    .with_context(|| format!("write {}", reviewer_index_json.display()))?;
    fs::write(&complete_ok, b"ok\n").with_context(|| format!("write {}", complete_ok.display()))?;

    Ok(HeroJudgeSeriesSummary {
        series_id: series_id.to_string(),
        output_dir: series_dir,
        run_count: runs.len(),
        runs,
        run_summaries_jsonl,
        quality_metrics_jsonl,
        quality_metrics_csv,
        lane_metrics_jsonl,
        lane_metrics_csv,
        hero_metrics_csv,
        judge_metrics_csv,
        series_summary_csv,
        reviewer_index_json,
        complete_ok,
    })
}
