//! Runtime for ZYAL Hero/Judge prompt evolution.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use jekko_store::db::Db;

use crate::hero_judge::HeroJudgeRunbook;
use crate::hero_judge_eval::{validate_config, zyal_yaml_body};
use crate::hero_judge_runner_flow::run_hero_judge_run_with_db as run_hero_judge_run_with_db_impl;
use crate::model_client::ModelClient;

/// Parse a ZYAL Hero/Judge runbook.
pub fn read_hero_judge_runbook(path: &Path) -> Result<HeroJudgeRunbook> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let yaml = zyal_yaml_body(&text).with_context(|| format!("parse {}", path.display()))?;
    let runbook: HeroJudgeRunbook =
        serde_yaml::from_str(&yaml).with_context(|| format!("decode {}", path.display()))?;
    validate_config(&runbook.hero_judge)?;
    Ok(runbook)
}

/// Run one Hero/Judge evolution workflow with the default DB.
pub async fn run_hero_judge_run(
    repo: &Path,
    run_id: &str,
    zyal_path: &Path,
    runbook: HeroJudgeRunbook,
    max_generations: Option<usize>,
    live_search: bool,
    model_client: &dyn ModelClient,
) -> Result<crate::hero_judge::HeroJudgeRunSummary> {
    let db = crate::daemon_store::open_db(repo)?;
    run_hero_judge_run_with_db(
        repo,
        run_id,
        zyal_path,
        runbook,
        max_generations,
        live_search,
        model_client,
        &db,
    )
    .await
}

/// Run one Hero/Judge evolution workflow with a caller-supplied DB.
#[allow(clippy::too_many_arguments)]
pub async fn run_hero_judge_run_with_db(
    repo: &Path,
    run_id: &str,
    zyal_path: &Path,
    runbook: HeroJudgeRunbook,
    max_generations: Option<usize>,
    live_search: bool,
    model_client: &dyn ModelClient,
    db: &Db,
) -> Result<crate::hero_judge::HeroJudgeRunSummary> {
    run_hero_judge_run_with_db_impl(
        repo,
        run_id,
        zyal_path,
        runbook,
        max_generations,
        live_search,
        model_client,
        db,
    )
    .await
}
