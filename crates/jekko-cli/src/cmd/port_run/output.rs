use anyhow::{anyhow, Result};
use serde::Serialize;
use zyal_supervisor::{execution_layers, SuperWorkflow};

use super::{open_store, PortRunArgs};

pub(super) fn emit_dry_run_plan(manifest: &SuperWorkflow, args: &PortRunArgs) -> Result<()> {
    let waves =
        execution_layers(manifest).map_err(|err| anyhow!("plan execution layers failed: {err}"))?;
    let synthetic_run_id = args
        .run_id
        .clone()
        .unwrap_or_else(|| format!("{}-dry-run", manifest.id));
    let plan = DryRunPlan {
        run_id: synthetic_run_id,
        manifest_id: manifest.id.clone(),
        manifest_name: manifest.name.clone(),
        waves,
        max_stages: args.max_stages,
        time_budget_hours: args.time_budget_hours,
    };
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

#[derive(Serialize)]
struct DryRunPlan {
    run_id: String,
    manifest_id: String,
    manifest_name: String,
    waves: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_stages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_budget_hours: Option<f64>,
}

pub(super) fn run_status(args: &PortRunArgs, run_id: &str) -> Result<()> {
    let store = open_store(args, false)?;
    let conn = store.connection();

    let mut phase_stmt = conn.prepare(
        "SELECT phase_id, name, objective, depends_on_json, status, summary, \
                started_at, completed_at, updated_at \
         FROM zyal_super_phases WHERE run_id = ?1 ORDER BY phase_id",
    )?;
    let phase_rows = phase_stmt
        .query_map([run_id], |row| {
            let depends_json: String = row.get(3)?;
            // A persisted phase row should always carry a valid JSON array
            // for `depends_on`. If parse fails, treat it as a corruption
            // signal (empty depends list = "no dependencies") and surface
            // it via stderr rather than silently default. Strict typed
            // state, no fallback-soup.
            let depends_on: Vec<String> = match serde_json::from_str::<Vec<String>>(&depends_json) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!(
                        "port-run --status: phase {phase} has malformed depends_on JSON: {err}",
                        phase = row.get::<_, String>(0).unwrap_or_else(|_| "?".to_string()),
                    );
                    Vec::new()
                }
            };
            Ok(PhaseStatusRow {
                phase_id: row.get(0)?,
                name: row.get(1)?,
                objective: row.get(2)?,
                depends_on,
                status: row.get(4)?,
                summary: row.get(5)?,
                started_at: row.get(6)?,
                completed_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut task_stmt = conn.prepare(
        "SELECT task_id, phase_id, title, status, owner, summary, updated_at \
         FROM zyal_super_tasks WHERE run_id = ?1 ORDER BY phase_id, task_id",
    )?;
    let task_rows = task_stmt
        .query_map([run_id], |row| {
            Ok(TaskStatusRow {
                task_id: row.get(0)?,
                phase_id: row.get(1)?,
                title: row.get(2)?,
                status: row.get(3)?,
                owner: row.get(4)?,
                summary: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let report = StatusReport {
        run_id: run_id.to_string(),
        phases: phase_rows,
        tasks: task_rows,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[derive(Serialize)]
struct StatusReport {
    run_id: String,
    phases: Vec<PhaseStatusRow>,
    tasks: Vec<TaskStatusRow>,
}

#[derive(Serialize)]
struct PhaseStatusRow {
    phase_id: String,
    name: String,
    objective: String,
    depends_on: Vec<String>,
    status: String,
    summary: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    updated_at: String,
}

#[derive(Serialize)]
struct TaskStatusRow {
    task_id: String,
    phase_id: String,
    title: String,
    status: String,
    owner: Option<String>,
    summary: String,
    updated_at: String,
}
