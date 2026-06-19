use anyhow::Result;
use jekko_store::daemon::{self, PortPhaseRow, PortTargetRow, PortTaskRow};
use jekko_store::db::Db;
use serde_json::json;

use crate::port::{PortMasterPlan, PortRuntimeOptions, PortTargetRequest};

use super::helpers::{hash_json, now_ms, target_id};

/// Persist target, phase, and task rows for a draft master plan.
pub fn persist_master_plan(db: &Db, run_id: &str, plan: &PortMasterPlan) -> Result<()> {
    let conn = db.connection();
    let now = now_ms();
    let target_id = target_id(run_id);
    if let Some(mut row) = daemon::get_run(conn, run_id)? {
        if !row.spec_json.is_object() {
            row.spec_json = json!({ "legacy_spec": row.spec_json });
        }
        if let Some(spec) = row.spec_json.as_object_mut() {
            spec.insert("master_plan".to_string(), serde_json::to_value(plan)?);
        }
        row.spec_hash = hash_json(&row.spec_json)?;
        row.time_updated = now;
        daemon::upsert_run(conn, &row)?;
    }
    daemon::upsert_port_target(
        conn,
        &PortTargetRow {
            id: target_id.clone(),
            run_id: run_id.to_string(),
            target: plan.target.target.clone(),
            replacement: plan.target.replacement.clone(),
            target_repo: plan.target.target_repo.clone(),
            replacement_repo: plan.target.replacement_repo.clone(),
            request: plan.target.request.clone(),
            status: "planned".to_string(),
            current_phase_id: plan.stages.first().map(|stage| stage.id.clone()),
            worker_cap: plan.target.effective_worker_cap() as i64,
            last_audit_score: None,
            last_parity_report_json: None,
            last_perf_gap_json: None,
            rollback_status: "clean".to_string(),
            quarantine_status: "none".to_string(),
            time_created: now,
            time_updated: now,
        },
    )?;
    for stage in &plan.stages {
        let task_ids = plan
            .tasks
            .iter()
            .filter(|task| task.stage_id == stage.id)
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        let task_count = plan
            .tasks
            .iter()
            .filter(|task| task.stage_id == stage.id)
            .count() as i64;
        daemon::upsert_port_phase(
            conn,
            &PortPhaseRow {
                id: stage.id.clone(),
                run_id: run_id.to_string(),
                target_id: target_id.clone(),
                ordinal: stage.ordinal as i64,
                name: stage.name.clone(),
                status: serde_json::to_string(&stage.status)?
                    .trim_matches('"')
                    .to_string(),
                strategy: "brainstorm_then_finalize".to_string(),
                plan_json: Some(json!({
                    "stage": stage,
                    "task_ids": task_ids,
                    "task_count": task_count,
                })),
                task_count,
                last_audit_score: None,
                last_parity_report_json: None,
                time_created: now,
                time_updated: now,
            },
        )?;
    }
    for task in &plan.tasks {
        daemon::upsert_port_task(
            conn,
            &PortTaskRow {
                id: task.id.clone(),
                run_id: run_id.to_string(),
                phase_id: task.stage_id.clone(),
                title: task.title.clone(),
                status: serde_json::to_string(&task.status)?
                    .trim_matches('"')
                    .to_string(),
                worker_id: None,
                branch: None,
                write_scope: task.write_scope.clone(),
                proof_lane: Some(task.proof_lane.clone()),
                attempt_count: 0,
                rollback_status: "clean".to_string(),
                quarantine_reason: None,
                last_error: None,
                time_created: now,
                time_updated: now,
            },
        )?;
    }
    Ok(())
}

/// Persist one fake worker completion for deterministic CI coverage.
pub fn persist_fake_worker_pass(
    db: &Db,
    run_id: &str,
    plan: &PortMasterPlan,
) -> Result<Option<String>> {
    let Some(task) = plan.tasks.first() else {
        return Ok(None);
    };
    let conn = db.connection();
    let now = now_ms();
    daemon::upsert_port_task(
        conn,
        &PortTaskRow {
            id: task.id.clone(),
            run_id: run_id.to_string(),
            phase_id: task.stage_id.clone(),
            title: task.title.clone(),
            status: "done".to_string(),
            worker_id: Some("fake-worker-1".to_string()),
            branch: Some(format!("zyal/{run_id}/fake-worker-1/{}", task.id)),
            write_scope: task.write_scope.clone(),
            proof_lane: Some(task.proof_lane.clone()),
            attempt_count: 1,
            rollback_status: "clean".to_string(),
            quarantine_reason: None,
            last_error: None,
            time_created: now,
            time_updated: now,
        },
    )?;
    Ok(Some(task.id.clone()))
}

/// Convert a port target request into a daemon spec payload.
pub fn port_spec(target: &PortTargetRequest) -> serde_json::Value {
    json!({
        "kind": "zyal_port",
        "target": target,
    })
}

/// Convert a port target request and runtime options into a daemon spec payload.
pub fn port_spec_with_runtime(
    target: &PortTargetRequest,
    runtime: &PortRuntimeOptions,
) -> serde_json::Value {
    json!({
        "kind": "zyal_port",
        "target": target,
        "evidence_inputs": &runtime.evidence_inputs,
        "live_call_budget": &runtime.live_call_budget,
        "proofs": &runtime.proofs,
        "model_policy": &runtime.model_policy,
    })
}
