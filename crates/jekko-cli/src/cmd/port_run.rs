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

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use zyal_supervisor::{validate_manifest, SuperWorkflow, SupervisorStore};

use crate::cli::GlobalOpts;

mod args;
mod output;
mod parse;
mod walk;

use args::{gate_live_mode, validate_arg_combination};
use output::{emit_dry_run_plan, run_status};
use parse::{load_manifest, open_store};
use walk::walk_waves;
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

    /// Backfill `summary.json` + `summary.md` for an existing run directory
    /// without touching the supervisor store or any live state. Reads
    /// `target/zyal/runs/<RUN_ID>/events.jsonl` and adjacent artifacts and
    /// writes the GOD-level run summary in place. Mutually exclusive with
    /// `--super`/`--resume`/`--status`.
    #[arg(long = "summarize", value_name = "RUN_ID")]
    pub summarize: Option<String>,
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

    if let Some(run_id) = args.summarize.as_deref() {
        return run_summarize(args, run_id);
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

/// Backfill summary.{json,md} for an existing run dir. Read-only against
/// the supervisor store; reads only events.jsonl + adjacent artifacts and
/// writes summary.json/summary.md in-place.
fn run_summarize(_args: &PortRunArgs, run_id: &str) -> Result<()> {
    use std::path::PathBuf;
    let run_dir: PathBuf = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target/zyal/runs")
        .join(run_id);
    if !run_dir.exists() {
        bail!(
            "run dir not found: {}. Did you mean a different run id?",
            run_dir.display()
        );
    }
    let summary = jankurai_runner::run_summary::build_and_write(&run_dir)?;
    println!(
        "{{\"run_id\":\"{}\",\"terminal_status\":\"{}\",\"summary_json\":\"{}\",\"summary_md\":\"{}\"}}",
        summary.run_id,
        summary.terminal_status,
        run_dir.join("summary.json").display(),
        run_dir.join("summary.md").display(),
    );
    Ok(())
}

/// Initialize a fresh run row from `manifest`. Honors an explicit `--run-id`
/// when supplied; otherwise the store derives `{manifest.id}-{millis}`.
fn init_or_use_run_id(
    store: &SupervisorStore,
    manifest: &SuperWorkflow,
    requested: Option<&str>,
) -> Result<String> {
    store
        .init_run(manifest, requested)
        .context("init supervisor run row")
}

fn run_resume(args: &PortRunArgs, run_id: &str) -> Result<()> {
    let store = open_store(args, false)?;
    let manifest = store
        .manifest_for_run(run_id)
        .with_context(|| format!("look up run `{run_id}`"))?
        .ok_or_else(|| anyhow!("run `{run_id}` not found"))?;

    store
        .reset_running_phases(run_id)
        .context("reset Running phases to Pending on resume")?;

    walk_waves(&store, &manifest, run_id, args)
}
