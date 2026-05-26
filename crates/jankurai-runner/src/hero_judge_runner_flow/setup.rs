use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::hero_judge::{HeroJudgeRunSummary, HeroJudgeRunbook};
use crate::hero_judge_eval::{run_objective, write_json_pretty};
use crate::hero_judge_runner_helpers::source_runbook_sha256;
use crate::hero_judge_search::{load_hero_judge_evidence, run_research};
use crate::model_client::ModelClient;

use super::generation::{run_generations, GenerationInputs};

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
) -> Result<HeroJudgeRunSummary> {
    let config = runbook.hero_judge.clone();
    let generations = config.effective_generations(max_generations);
    let lane_parallelism = config
        .population
        .max_parallel
        .min(config.super_reasoning.effective_max_workers())
        .max(1);
    let source_runbook_sha256 = source_runbook_sha256(zyal_path, &runbook)?;
    let output_dir = repo.join(config.output_root()).join(run_id);
    fs::create_dir_all(&output_dir).with_context(|| format!("mkdir {}", output_dir.display()))?;
    let sink = EventSink::open(repo, run_id)?;
    daemon_store::ensure_daemon_run(
        db,
        repo,
        run_id,
        json!({
            "kind": "zyal_hero_judge",
            "zyal_path": zyal_path.display().to_string(),
            "source_runbook_sha256": source_runbook_sha256,
            "hero_judge": config,
            "super_reasoning": {
                "enabled": runbook.hero_judge.super_reasoning.enabled,
                "effective_max_workers": runbook.hero_judge.super_reasoning.effective_max_workers(),
                "credential_policy": runbook.hero_judge.super_reasoning.credential_policy,
            },
            "model_policy": runbook.hero_judge.model_policy,
            "live_call_budget": {
                "max_calls": runbook.hero_judge.budgets.model_calls,
                "max_parallel": lane_parallelism,
                "require_live": false,
            },
        }),
    )?;
    sink.emit(
        EventKind::RunStarted,
        json!({
            "workflow": "zyal_hero_judge",
            "generations": generations,
            "live_model_calls": live_search,
            "credential_policy": config.super_reasoning.credential_policy.env_value(),
            "mock_llm_set": std::env::var_os("JEKKO_TUI_TEST_MOCK_LLM").is_some(),
        }),
    )?;

    let evidence = load_hero_judge_evidence(repo, &config)?;
    let objective = run_objective(&runbook, &config);
    let search_receipts = run_research(repo, &objective, &config, live_search).await?;
    write_json_pretty(
        &output_dir.join("search").join("receipts.json"),
        &search_receipts,
    )?;
    for receipt in &search_receipts {
        sink.emit(
            EventKind::ResearchReceipt,
            json!({"id": receipt.id, "provider": receipt.provider, "status": receipt.status}),
        )?;
    }

    let state = run_generations(GenerationInputs {
        repo,
        run_id,
        db,
        sink: &sink,
        model_client,
        config: &config,
        objective: &objective,
        evidence: &evidence,
        search_receipts: &search_receipts,
        output_dir: &output_dir,
        generations,
        lane_parallelism,
        require_parsed_live_json: live_search,
    })
    .await?;

    crate::hero_judge_runner_finalize::finalize_run(
        crate::hero_judge_runner_finalize::FinalizeInputs {
            repo,
            run_id,
            db,
            sink: &sink,
            config: &config,
            source_runbook_sha256,
            objective,
            output_dir,
            generations,
            lane_parallelism,
            model_calls_used: state.model_calls_used,
            last_model_kind: state.last_model_kind,
            last_decision: state.last_decision,
            prompt_lineage: state.prompt_lineage,
            scoreboard: state.scoreboard,
            knowledge: state.knowledge,
            quality_metrics: state.quality_metrics,
            lane_metrics: state.lane_metrics,
            reviewer_cards: state.reviewer_cards,
            search_receipts,
        },
    )
}
