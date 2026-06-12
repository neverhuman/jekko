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
    let report = store.status_report(run_id)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
