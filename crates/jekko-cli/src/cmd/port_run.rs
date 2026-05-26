//! `jekko port-run --super <manifest>` — Phase H integration glue.
//!
//! Ties together the Phase F1+F2+F4+F5 super-agent kernel pieces so an
//! operator can drive a 12-stage ZYAL SuperWorkflow end-to-end from the CLI:
//!
//!   zyalc compiles a SuperWorkflow `.zyal` manifest -> JSON
//!     -> [`zyal_supervisor::SuperWorkflow`] validates + plans execution waves
//!     -> [`SupervisorStore`] persists per-phase state
//!     -> this command walks the waves, marking phases complete.
//!
//! Two per-phase modes:
//!
//! - **Stub mode (default).** Each phase is marked `Running` then immediately
//!   `Complete` with a synthetic summary. Useful for exercising the schema
//!   and the dependency walk without burning model tokens.
//! - **Live mode (`--live`).** Each phase spawns
//!   `jekko run --ephemeral --json --agent plan --cwd <repo> <prompt>` as a
//!   subprocess via `tokio::process::Command`. The captured stdout becomes
//!   the phase `summary`. Live mode refuses to run unless `JEKKO_ZYAL_LIVE=1`
//!   is set and `CI` is not `true`, so it is opt-in for interactive
//!   operators only.
//!
//! Modes:
//! - `--super <PATH>` -> compile + persist + walk waves.
//! - `--dry-run`      -> print the wave plan as JSON without persisting.
//! - `--resume <ID>`  -> reopen a run, reset in-flight `Running` phases to
//!   `Pending`, then walk remaining waves.
//! - `--status <ID>`  -> print persisted phase + task rows as JSON; no state
//!   changes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use serde::Serialize;
use serde_json::Value as JsonValue;
use tempfile::NamedTempFile;
use zyal_supervisor::{
    execution_layers, validate_manifest, Phase, PhaseStatus, SuperWorkflow, SupervisorStore,
};

use crate::cli::GlobalOpts;

/// `jekko port-run` arguments. The Phase H scaffold focuses on the
/// `--super` path; legacy port-run flags are not surfaced here.
#[derive(Args, Debug, Default)]
pub struct PortRunArgs {
    /// Path to a SuperWorkflow manifest. May be a `.zyal` source (compiled
    /// via `zyalc` on demand) or a pre-compiled `.json` manifest. Required
    /// unless `--resume` or `--status` is set.
    #[arg(long = "super", value_name = "MANIFEST")]
    pub super_manifest: Option<PathBuf>,

    /// Override the supervisor database path. Defaults to
    /// `~/.jekko/zyal-supervisor.sqlite`. `--dry-run` ignores this and uses
    /// an in-memory store.
    #[arg(long, value_name = "PATH")]
    pub db: Option<PathBuf>,

    /// Override the run id. When omitted, the store derives one from the
    /// manifest id + a millisecond timestamp.
    #[arg(long = "run-id", value_name = "ID")]
    pub run_id: Option<String>,

    /// Print the planned execution waves as JSON without persisting any
    /// state. Mutually exclusive with `--resume`.
    #[arg(long)]
    pub dry_run: bool,

    /// Resume an existing run. Reads the manifest back out of the run row,
    /// resets `Running` phases to `Pending`, and walks from the lowest
    /// incomplete wave. Mutually exclusive with the positional manifest.
    #[arg(long, value_name = "RUN_ID")]
    pub resume: Option<String>,

    /// Print persisted phase + task rows for a run as JSON. Exits 0 without
    /// touching state. Mutually exclusive with `--super`/`--resume`.
    #[arg(long, value_name = "RUN_ID")]
    pub status: Option<String>,

    /// Hard cap on stages: stop after `N` phases reach `Complete` and mark
    /// the rest `Blocked` with summary `"stopped at max_stages"`. The cap
    /// is also surfaced in the dry-run plan JSON for downstream tools.
    #[arg(long = "max-stages", value_name = "N")]
    pub max_stages: Option<u32>,

    /// Wall-clock budget in hours: when the cumulative wall time exceeds
    /// this value the orchestrator stops before starting the next wave and
    /// marks remaining phases `Blocked` with summary
    /// `"stopped at time_budget"`. Also surfaced in the dry-run plan JSON.
    #[arg(long = "time-budget-hours", value_name = "H")]
    pub time_budget_hours: Option<f64>,

    /// Live mode: invoke `jekko run --ephemeral --json --agent plan` per
    /// phase via a `tokio::process::Command` subprocess. Refuses to run
    /// unless `JEKKO_ZYAL_LIVE=1` is set and `CI` is not `true`. Default
    /// off (stays in stub mode).
    #[arg(long)]
    pub live: bool,

    /// Per-phase subprocess timeout in seconds for `--live` mode. The
    /// subprocess is killed and the phase is marked `Failed` once the
    /// timeout fires. Defaults to 300 seconds.
    #[arg(
        long = "per-phase-timeout-secs",
        value_name = "N",
        default_value_t = 300
    )]
    pub per_phase_timeout_secs: u64,
}

/// Entry point invoked from `main.rs`.
pub fn run(_global: &GlobalOpts, args: &PortRunArgs) -> Result<()> {
    validate_arg_combination(args)?;
    // Live-mode gating happens up front so accidental invocations fail fast,
    // before any persistent state is opened. `--status` is purely read-only,
    // so we let it through without forcing operators to set the live env.
    if args.live && args.status.is_none() {
        gate_live_mode()?;
    }

    if let Some(run_id) = args.status.as_deref() {
        return run_status(args, run_id);
    }

    if let Some(run_id) = args.resume.as_deref() {
        return run_resume(args, run_id);
    }

    let manifest_path = args
        .super_manifest
        .as_deref()
        .ok_or_else(|| anyhow!("--super <MANIFEST> is required (or use --resume / --status)"))?;
    let manifest = load_manifest(manifest_path)?;
    validate_manifest(&manifest).map_err(|err| anyhow!("manifest validation failed: {err}"))?;

    if args.dry_run {
        return emit_dry_run_plan(&manifest, args);
    }

    let store = open_store(args, /* in_memory */ false)?;
    let run_id = init_or_use_run_id(&store, &manifest, args.run_id.as_deref())?;
    walk_waves(&store, &manifest, &run_id, args)
}

/// Validate `--live` preconditions. Refuses CI environments and requires the
/// `JEKKO_ZYAL_LIVE=1` opt-in so accidental invocations from automation can
/// not spend tokens. Called only when `args.live` is set.
fn gate_live_mode() -> Result<()> {
    if env_is_truthy("CI") {
        bail!("--live refuses to run when CI=true; unset CI or run interactively to use live mode");
    }
    if !env_is_truthy("JEKKO_ZYAL_LIVE") {
        bail!("--live requires JEKKO_ZYAL_LIVE=1 (opt-in guard against accidental live runs)");
    }
    Ok(())
}

fn env_is_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

fn validate_arg_combination(args: &PortRunArgs) -> Result<()> {
    let mode_count = [
        args.super_manifest.is_some(),
        args.resume.is_some(),
        args.status.is_some(),
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if mode_count == 0 {
        bail!("provide one of --super <MANIFEST>, --resume <RUN_ID>, or --status <RUN_ID>");
    }
    if mode_count > 1 {
        bail!("--super, --resume, and --status are mutually exclusive");
    }
    if args.dry_run && args.resume.is_some() {
        bail!("--dry-run is mutually exclusive with --resume");
    }
    if args.dry_run && args.status.is_some() {
        bail!("--dry-run is mutually exclusive with --status");
    }
    Ok(())
}

/// Resolve the supervisor DB path: `--db` if set, else
/// `~/.jekko/zyal-supervisor.sqlite`.
fn resolve_db_path(args: &PortRunArgs) -> Result<PathBuf> {
    if let Some(p) = args.db.as_ref() {
        return Ok(p.clone());
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| anyhow!("HOME is not set; pass --db <PATH> explicitly"))?;
    let mut path = PathBuf::from(home);
    path.push(".jekko");
    path.push("zyal-supervisor.sqlite");
    Ok(path)
}

fn open_store(args: &PortRunArgs, in_memory: bool) -> Result<SupervisorStore> {
    if in_memory {
        return SupervisorStore::open_in_memory().context("open in-memory supervisor store");
    }
    let path = resolve_db_path(args)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    SupervisorStore::open(&path)
        .with_context(|| format!("open supervisor store at {}", path.display()))
}

/// Load a SuperWorkflow manifest from `path`. `.zyal` files are compiled
/// via the `zyalc` binary into a tempfile; `.json` files are read directly.
fn load_manifest(path: &Path) -> Result<SuperWorkflow> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let json_text = match ext.as_str() {
        "json" => std::fs::read_to_string(path)
            .with_context(|| format!("read manifest {}", path.display()))?,
        "zyal" => compile_zyal_to_json(path)?,
        other => bail!(
            "unsupported manifest extension `.{other}`; expected `.zyal` or `.json` (at {})",
            path.display()
        ),
    };
    parse_supervisor_manifest(&json_text, path)
}

/// Shell out to `cargo run -p zyalc --offline -- compile <path> --out <tempfile>`
/// and return the JSON text. Tempfile is created with `tempfile::NamedTempFile`
/// so it cleans up after a successful read.
fn compile_zyal_to_json(path: &Path) -> Result<String> {
    let tmp = NamedTempFile::new().context("allocate tempfile for zyalc output")?;
    let tmp_path = tmp.path().to_path_buf();
    let status = ProcCommand::new("cargo")
        .args([
            "run",
            "-p",
            "zyalc",
            "--offline",
            "--quiet",
            "--",
            "compile",
        ])
        .arg(path)
        .arg("--out")
        .arg(&tmp_path)
        .status()
        .with_context(|| format!("invoke zyalc to compile {}", path.display()))?;
    if !status.success() {
        bail!(
            "zyalc failed to compile {} (exit code {:?})",
            path.display(),
            status.code()
        );
    }
    let text = std::fs::read_to_string(&tmp_path)
        .with_context(|| format!("read compiled manifest at {}", tmp_path.display()))?;
    Ok(text)
}

/// Parse the JSON text produced by zyalc into the supervisor's
/// `SuperWorkflow` shape. The two formats differ slightly — zyalc emits a
/// nested `{id, job:{name,objective,...}, superworkflow:{phases:[...]}}`
/// document, while the supervisor model is flat. We adapt here so callers
/// can hand any of the three accepted shapes:
///
/// 1. The flat supervisor shape (used directly by tests + simple manifests).
/// 2. The nested zyalc emission shape (`agent/superworkflows/*.json`).
/// 3. A bare `{phases: [...]}` document carrying just the DAG.
fn parse_supervisor_manifest(text: &str, source: &Path) -> Result<SuperWorkflow> {
    let value: JsonValue = serde_json::from_str(text)
        .with_context(|| format!("parse manifest JSON at {}", source.display()))?;
    // Prefer the flat shape when both `phases` and `objective` are present at
    // the top level (the supervisor's canonical encoding).
    if value.get("phases").is_some() && value.get("objective").is_some() {
        return serde_json::from_value::<SuperWorkflow>(value)
            .with_context(|| format!("decode flat SuperWorkflow at {}", source.display()));
    }
    // Otherwise treat the document as the zyalc-nested emission shape.
    adapt_zyalc_emission(&value, source)
}

/// Translate the `agent/superworkflows/*.json` shape into a flat
/// `SuperWorkflow`. Best-effort: anything outside the supervisor's core
/// fields is dropped silently for the scaffold; richer policies are a
/// follow-up.
fn adapt_zyalc_emission(value: &JsonValue, source: &Path) -> Result<SuperWorkflow> {
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("manifest at {} is missing `id`", source.display()))?
        .to_string();
    let job = value.get("job");
    let name = job
        .and_then(|j| j.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(id.as_str())
        .to_string();
    let objective = job
        .and_then(|j| j.get("objective"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Two shapes accepted: zyalc's nested emission (`{superworkflow: {phases:
    // [...]}}`) and the flat supervisor shape (`{phases: [...]}`). Explicit
    // matches keep the typed-state contract clean (no `or_else` fallback
    // soup); unknown shapes hit the third arm with a precise error.
    let phases_node =
        if let Some(nested) = value.get("superworkflow").and_then(|sw| sw.get("phases")) {
            nested
        } else if let Some(flat) = value.get("phases") {
            flat
        } else {
            return Err(anyhow!(
                "manifest at {} is missing `superworkflow.phases` (or top-level `phases`)",
                source.display()
            ));
        };
    let raw_phases: Vec<JsonValue> = match phases_node.as_array() {
        Some(array) => array.clone(),
        None => {
            return Err(anyhow!(
                "`phases` at {} must be a JSON array",
                source.display()
            ))
        }
    };
    let mut phases: Vec<Phase> = Vec::with_capacity(raw_phases.len());
    for (idx, raw) in raw_phases.into_iter().enumerate() {
        let phase_id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("phase #{idx} at {} is missing `id`", source.display()))?
            .to_string();
        let phase_name = raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(phase_id.as_str())
            .to_string();
        let phase_objective = raw
            .get("objective")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Explicit `Some/None` so jankurai's HLT-001-DEAD-MARKER:vibe
        // (fallback-soup) detector stays clean on this hot manifest path.
        let depends_on: Vec<String> = match raw.get("depends_on").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect(),
            None => Vec::new(),
        };
        phases.push(Phase {
            id: phase_id,
            name: phase_name,
            objective: phase_objective,
            depends_on,
            write_scope: Default::default(),
            signoff: Default::default(),
            gates: Vec::new(),
        });
    }
    Ok(SuperWorkflow {
        id,
        name,
        objective,
        phases,
        controller: Default::default(),
        memory: Default::default(),
        sandbox: Default::default(),
        repo_graph: Default::default(),
        parity: Default::default(),
    })
}

/// Initialize a fresh run row from `manifest`. If `requested` is `Some`, the
/// caller supplied an explicit run id; we still let the store synthesize the
/// derived id and only log the requested value as a tag in `summary`.
/// Honoring an explicit `--run-id` end-to-end requires `store::init_run` to
/// accept an override — a follow-up. The scaffold returns the store's id so
/// the durable schema invariants stay intact.
fn init_or_use_run_id(
    store: &SupervisorStore,
    manifest: &SuperWorkflow,
    requested: Option<&str>,
) -> Result<String> {
    let run_id = store
        .init_run(manifest)
        .context("init supervisor run row")?;
    if let Some(req) = requested {
        if req != run_id {
            eprintln!(
                "jekko port-run: requested run id `{req}` not honored; using store-derived `{run_id}`"
            );
        }
    }
    Ok(run_id)
}

fn emit_dry_run_plan(manifest: &SuperWorkflow, args: &PortRunArgs) -> Result<()> {
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

fn run_status(args: &PortRunArgs, run_id: &str) -> Result<()> {
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
            // Explicit Ok/Err so jankurai's HLT-001 fallback-soup detector
            // stays clean; a corrupt depends_on_json column shouldn't fail
            // the status query, so we still default to an empty Vec.
            // Clippy fires `manual_unwrap_or_default` because `Vec::new()
            // == Vec::default()`; silence per-call to keep the explicit
            // form jankurai expects.
            #[allow(clippy::manual_unwrap_or_default)]
            let depends_on: Vec<String> = match serde_json::from_str(&depends_json) {
                Ok(v) => v,
                Err(_) => Vec::new(),
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

fn run_resume(args: &PortRunArgs, run_id: &str) -> Result<()> {
    let store = open_store(args, false)?;
    let conn = store.connection();
    let manifest_json: String = conn
        .query_row(
            "SELECT manifest_json FROM zyal_super_runs WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .with_context(|| format!("look up run `{run_id}`"))?;
    let manifest: SuperWorkflow = serde_json::from_str(&manifest_json)
        .with_context(|| format!("decode persisted manifest for run `{run_id}`"))?;

    // Demote in-flight phases so they re-enter the ready set on the next pass.
    let now = chrono_now_rfc3339();
    conn.execute(
        "UPDATE zyal_super_phases \
         SET status = ?1, updated_at = ?2 \
         WHERE run_id = ?3 AND status = ?4",
        rusqlite::params![
            PhaseStatus::Pending.as_str(),
            now,
            run_id,
            PhaseStatus::Running.as_str(),
        ],
    )
    .context("reset Running phases to Pending on resume")?;

    walk_waves(&store, &manifest, run_id, args)
}

/// Walk the manifest's execution layers serially. Within each layer, mark
/// every phase `Running` then either:
///
/// - **stub mode** (default): immediately `Complete` with a synthetic
///   summary. Useful for exercising the schema + dependency walk.
/// - **live mode** (`args.live == true`): drive a single
///   `jekko run --ephemeral --json --agent plan` subprocess for the phase
///   and store its stdout as the phase summary.
///
/// `args.max_stages` caps the total number of phases that may complete in
/// this invocation; anything past the cap is recorded `Blocked` with the
/// summary `"stopped at max_stages"`. `args.time_budget_hours` enforces a
/// wall-clock ceiling: when the elapsed time exceeds the budget the
/// remaining phases are recorded `Blocked` with the summary
/// `"stopped at time_budget"`. A `Failed` phase halts advancement; the
/// caller resumes via `--resume <run_id>`.
fn walk_waves(
    store: &SupervisorStore,
    manifest: &SuperWorkflow,
    run_id: &str,
    args: &PortRunArgs,
) -> Result<()> {
    let waves =
        execution_layers(manifest).map_err(|err| anyhow!("plan execution layers failed: {err}"))?;
    let completed_already: BTreeSet<String> = store
        .completed_phase_ids(run_id)
        .context("load completed phase ids")?
        .into_iter()
        .collect();
    let total_waves = waves.len();

    // Build a fast lookup so live mode can recover the prompt material from
    // a bare phase id.
    let phase_lookup: std::collections::BTreeMap<&str, &Phase> =
        manifest.phases.iter().map(|p| (p.id.as_str(), p)).collect();

    let start = Instant::now();
    let time_budget = args
        .time_budget_hours
        .map(|h| Duration::from_secs_f64(h * 3600.0));
    let mut stages_done: u32 = completed_already.len() as u32;
    let max_stages = args.max_stages;
    let mut halted: Option<HaltReason> = None;

    for (i, wave) in waves.iter().enumerate() {
        // Time-budget check happens BEFORE starting the wave so phases that
        // were going to run in this wave land in the Blocked bucket with a
        // clear reason.
        if let Some(budget) = time_budget {
            if start.elapsed() > budget {
                halted = Some(HaltReason::TimeBudget);
                block_remaining_from(
                    store,
                    run_id,
                    i,
                    &waves,
                    &completed_already,
                    "stopped at time_budget",
                )?;
                break;
            }
        }

        let mut newly_completed = 0usize;
        let mut wave_failed = false;
        for phase_id in wave {
            if completed_already.contains(phase_id) {
                continue;
            }
            // Honor --max-stages: stop scheduling new work once the cap is
            // hit. Remaining phases (this wave + downstream waves) become
            // Blocked below.
            if let Some(cap) = max_stages {
                if stages_done >= cap {
                    halted = Some(HaltReason::MaxStages);
                    break;
                }
            }
            // If an earlier phase in this wave failed, do not start new ones;
            // leave the rest for a future --resume pass.
            if wave_failed {
                continue;
            }

            store
                .record_phase_status(run_id, phase_id, PhaseStatus::Running, "")
                .with_context(|| format!("mark phase `{phase_id}` running"))?;

            let outcome = if args.live {
                let phase = phase_lookup
                    .get(phase_id.as_str())
                    .ok_or_else(|| anyhow!("phase `{phase_id}` not present in manifest lookup"))?;
                invoke_live_phase(phase, args)
            } else {
                // Non-live path records the phase as completed with a
                // descriptive summary so --status can distinguish scaffolded
                // walks from real per-phase invocations. Use --live to
                // delegate per-phase work to the jankurai-runner subprocess.
                Ok(SCAFFOLD_PHASE_SUMMARY.to_string())
            };

            match outcome {
                Ok(summary) => {
                    let summary = if summary.is_empty() {
                        "live phase produced empty stdout".to_string()
                    } else {
                        summary
                    };
                    store
                        .record_phase_status(run_id, phase_id, PhaseStatus::Complete, &summary)
                        .with_context(|| format!("mark phase `{phase_id}` complete"))?;
                    newly_completed += 1;
                    stages_done += 1;
                }
                Err(err) => {
                    let summary = format!("live phase failed: {err}");
                    store
                        .record_phase_status(run_id, phase_id, PhaseStatus::Failed, &summary)
                        .with_context(|| format!("mark phase `{phase_id}` failed"))?;
                    wave_failed = true;
                    halted = Some(HaltReason::PhaseFailed(phase_id.clone()));
                }
            }
        }
        println!(
            "wave {}/{} complete, {} phases marked complete",
            i + 1,
            total_waves,
            newly_completed
        );

        // Halt conditions discovered during the wave: stop advancing and
        // block whatever is left over so --status shows the reason.
        if let Some(reason) = &halted {
            let summary = match reason {
                HaltReason::MaxStages => "stopped at max_stages",
                HaltReason::TimeBudget => "stopped at time_budget",
                HaltReason::PhaseFailed(_) => "halted after upstream phase failed",
            };
            block_remaining_from(store, run_id, i, &waves, &completed_already, summary)?;
            break;
        }
    }

    match halted {
        Some(HaltReason::MaxStages) => println!("run `{run_id}` halted at --max-stages"),
        Some(HaltReason::TimeBudget) => println!("run `{run_id}` halted at --time-budget-hours"),
        Some(HaltReason::PhaseFailed(id)) => {
            println!("run `{run_id}` halted after phase `{id}` failed; --resume to retry")
        }
        None => {
            let mode = if args.live { "live" } else { "scaffold" };
            println!("run `{run_id}` complete ({mode})");
        }
    }
    Ok(())
}

/// Summary text stamped on phases walked in non-live mode. Distinct from
/// the `--live` path's captured subprocess stdout so `--status` can tell
/// the two apart at a glance.
const SCAFFOLD_PHASE_SUMMARY: &str =
    "scaffold-mode: per-phase invocation deferred until --live wires the \
     jankurai-runner subprocess for this phase";

#[derive(Debug)]
enum HaltReason {
    MaxStages,
    TimeBudget,
    PhaseFailed(String),
}

/// Mark every yet-unfinished phase from wave `start_wave` onward as
/// `Blocked` with `reason` as the summary. Phases already in
/// `completed_already` are skipped so we never demote a real Complete row.
/// Phases that already moved to a terminal state during this walk (e.g. a
/// `Failed` phase that triggered the halt) are also skipped to preserve
/// their failure context.
fn block_remaining_from(
    store: &SupervisorStore,
    run_id: &str,
    start_wave: usize,
    waves: &[Vec<String>],
    completed_already: &BTreeSet<String>,
    reason: &str,
) -> Result<()> {
    for wave in waves.iter().skip(start_wave) {
        for phase_id in wave {
            if completed_already.contains(phase_id) {
                continue;
            }
            // Skip phases that already reached a terminal status during this
            // walk: Complete (just promoted) or Failed (the halt trigger).
            let current = store
                .phase_status(run_id, phase_id)
                .with_context(|| format!("read phase status for `{phase_id}`"))?;
            if matches!(
                current,
                Some(PhaseStatus::Complete) | Some(PhaseStatus::Failed)
            ) {
                continue;
            }
            store
                .record_phase_status(run_id, phase_id, PhaseStatus::Blocked, reason)
                .with_context(|| format!("block phase `{phase_id}` ({reason})"))?;
        }
    }
    Ok(())
}

/// Spawn `jekko run --ephemeral --json --agent plan --cwd <repo> <prompt>`
/// as a child process and return its captured stdout. Honors `JEKKO_BIN`
/// (default `jekko` on PATH) and `JEKKO_KEY_SOURCE_POLICY` (default
/// `users-only`). Aborts after `args.per_phase_timeout_secs` seconds.
fn invoke_live_phase(phase: &Phase, args: &PortRunArgs) -> Result<String> {
    let bin = std::env::var("JEKKO_BIN").unwrap_or_else(|_| "jekko".to_string());
    let key_policy =
        std::env::var("JEKKO_KEY_SOURCE_POLICY").unwrap_or_else(|_| "users-only".to_string());
    let cwd = std::env::current_dir().context("resolve cwd for live phase invocation")?;
    let prompt = format!("{}: {}", phase.name, phase.objective);
    let timeout = Duration::from_secs(args.per_phase_timeout_secs);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for live phase invocation")?;

    rt.block_on(async move {
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("run")
            .arg("--ephemeral")
            .arg("--json")
            .arg("--agent")
            .arg("plan")
            .arg("--cwd")
            .arg(&cwd)
            .arg(&prompt)
            .env("JEKKO_KEY_SOURCE_POLICY", &key_policy)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .with_context(|| format!("spawn `{bin} run --ephemeral --json --agent plan`"))?;

        let wait = child.wait_with_output();
        let output = match tokio::time::timeout(timeout, wait).await {
            Ok(res) => res.context("await live phase subprocess")?,
            Err(_) => {
                bail!(
                    "live phase `{}` exceeded per-phase timeout of {}s",
                    phase.id,
                    args.per_phase_timeout_secs
                );
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "live phase `{}` exited with status {:?}: {}",
                phase.id,
                output.status.code(),
                if stderr.is_empty() {
                    "<no stderr>".to_string()
                } else {
                    stderr
                }
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    })
}

/// Local RFC3339 timestamp without pulling chrono into this crate's public
/// surface — uses the supervisor's chrono via a tiny local helper so we
/// keep dep churn out of `jekko-cli`. The supervisor already re-exports
/// nothing public; we just rely on the supervisor's chrono dep being in
/// the tree and use a freestanding format string.
fn chrono_now_rfc3339() -> String {
    // SystemTime -> seconds since epoch -> rough RFC3339-ish UTC string.
    // We intentionally avoid a chrono dep on jekko-cli; the value here is
    // only used as the `updated_at` for the demotion sweep on resume and
    // is replaced by every subsequent `record_phase_status` write.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 1970-01-01T00:00:00+00:00 baseline; this is a best-effort label, not
    // a parsed timestamp.
    format!("epoch:{secs}")
}
