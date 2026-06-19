use std::path::Path;

use anyhow::{anyhow, Result};
use jekko_store::db::Db;
use serde_json::json;

use super::config::{PortRunConfig, PortTickReport};
use super::helpers::{
    assert_clean_tree, current_audit_snapshot, graph_summary_json, planning_prompt,
};
use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::jankurai_gate::{self, JankuraiGatePolicy};
use crate::model_client::ModelClient;
use crate::model_policy::ModelTaskKind;
use crate::port::{draft_master_plan, validate_master_plan_contract};
use crate::reasoning_runner::run_advanced_reasoning_tick_with_db;
use crate::repo_graph::build_repo_graph;

/// Run one durable port workflow tick.
pub async fn run_port_tick(
    repo: &Path,
    run_id: &str,
    config: PortRunConfig,
    model_client: &dyn ModelClient,
) -> Result<PortTickReport> {
    let db = daemon_store::open_db(repo)?;
    run_port_tick_with_db(repo, run_id, config, model_client, &db).await
}

/// Run one durable port workflow tick with a caller-supplied DB handle.
pub async fn run_port_tick_with_db(
    repo: &Path,
    run_id: &str,
    config: PortRunConfig,
    model_client: &dyn ModelClient,
    db: &Db,
) -> Result<PortTickReport> {
    if !config.allow_dirty {
        assert_clean_tree(repo)?;
    }
    if config.advanced_reasoning.enabled {
        return run_advanced(repo, run_id, config, model_client, db).await;
    }

    let sink = EventSink::open(repo, run_id)?;
    daemon_store::ensure_daemon_run(
        db,
        repo,
        run_id,
        daemon_store::port_spec_with_runtime(&config.target, &config.runtime),
    )?;
    emit_run_started(&sink, &config)?;

    let graph = build_repo_graph(repo)?;
    daemon_store::persist_repo_graph(db, run_id, &graph)?;
    let graph_summary = graph_summary_json(&graph)?;
    emit_brainstorm_started(&sink, &config, graph_summary.clone())?;

    let prompt = planning_prompt(&config.target, &graph);
    let model_receipt = model_client
        .complete(ModelTaskKind::PhaseFinalize, &prompt, repo)
        .await?;
    daemon_store::persist_model_receipt(db, run_id, &model_receipt)?;
    emit_model_outcome(&sink, &model_receipt)?;
    if !model_receipt.success {
        daemon_store::mark_daemon_run(
            db,
            run_id,
            "blocked",
            "model_planning",
            model_receipt.error.as_deref(),
        )?;
        return Err(anyhow!(
            "model planning failed: {}",
            model_receipt
                .error
                .as_deref()
                .unwrap_or("unknown model failure")
        ));
    }

    let plan = draft_master_plan(config.target.clone());
    validate_master_plan_contract(&plan)?;
    daemon_store::persist_master_plan(db, run_id, &plan)?;
    sink.emit(
        EventKind::PhaseFinalized,
        json!({"stage_count": plan.stages.len(), "task_count": plan.tasks.len()}),
    )?;
    let fake_task_completed =
        maybe_run_fake_worker(db, run_id, &plan, &sink, config.fake_worker_cycle)?;

    let audit = current_audit_snapshot(repo)?;
    jankurai_gate::check_gate(audit, audit, JankuraiGatePolicy::default())?;
    sink.emit(
        EventKind::AuditResult,
        json!({
            "score": audit.score,
            "hard_findings": audit.hard_findings,
            "caps": audit.caps,
            "status": "passed",
        }),
    )?;
    daemon_store::mark_daemon_run(db, run_id, "running", "phase_plan", None)?;

    Ok(PortTickReport {
        run_id: run_id.to_string(),
        target_id: daemon_store::target_id(run_id),
        plan,
        model_receipt,
        graph_summary,
        fake_task_completed,
        advanced_reasoning: None,
    })
}

async fn run_advanced(
    repo: &Path,
    run_id: &str,
    config: PortRunConfig,
    model_client: &dyn ModelClient,
    db: &Db,
) -> Result<PortTickReport> {
    let report = run_advanced_reasoning_tick_with_db(
        repo,
        run_id,
        config.target.clone(),
        config.advanced_reasoning.clone(),
        config.runtime.clone(),
        config.fake_worker_cycle,
        model_client,
        db,
    )
    .await?;
    Ok(PortTickReport {
        run_id: report.run_id,
        target_id: report.target_id,
        plan: report.plan,
        model_receipt: report.model_receipt,
        graph_summary: report.graph_summary,
        fake_task_completed: report.fake_task_completed,
        advanced_reasoning: Some(report.advanced),
    })
}

fn emit_run_started(sink: &EventSink, config: &PortRunConfig) -> Result<()> {
    sink.emit(
        EventKind::RunStarted,
        json!({
            "workflow": "zyal_port",
            "target": config.target.target,
            "replacement": config.target.replacement,
        }),
    )
}

fn emit_brainstorm_started(
    sink: &EventSink,
    config: &PortRunConfig,
    graph_summary: serde_json::Value,
) -> Result<()> {
    sink.emit(
        EventKind::BrainstormStarted,
        json!({
            "worker_cap": config.target.effective_worker_cap(),
            "graph": graph_summary,
        }),
    )
}

fn emit_model_outcome(
    sink: &EventSink,
    model_receipt: &crate::model_client::ModelCallReceipt,
) -> Result<()> {
    sink.emit(
        EventKind::ModelOutcome,
        json!({
            "kind": model_receipt.kind,
            "provider": model_receipt.provider,
            "model": model_receipt.model,
            "success": model_receipt.success,
        }),
    )
}

fn maybe_run_fake_worker(
    db: &Db,
    run_id: &str,
    plan: &crate::port::PortMasterPlan,
    sink: &EventSink,
    enabled: bool,
) -> Result<Option<String>> {
    if !enabled {
        return Ok(None);
    }
    let completed = daemon_store::persist_fake_worker_pass(db, run_id, plan)?;
    if let Some(task_id) = &completed {
        sink.emit(
            EventKind::TaskAssigned,
            json!({"task_id": task_id, "worker_id": "fake-worker-1"}),
        )?;
        sink.emit(
            EventKind::WorkerStarted,
            json!({"task_id": task_id, "worker_id": "fake-worker-1"}),
        )?;
        sink.emit(
            EventKind::WorkerPass,
            json!({"task_id": task_id, "worker_id": "fake-worker-1"}),
        )?;
        sink.emit(
            EventKind::ProofPassed,
            json!({"task_id": task_id, "lane": "fake"}),
        )?;
    }
    Ok(completed)
}
