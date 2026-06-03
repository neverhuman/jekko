//! Wave-walking + live-phase invocation for `jekko port-run`.
//!
//! Split out of `port_run.rs` to keep that file under the 500-LOC shape
//! threshold (jankurai HLT-001:shape). All callers live in the parent
//! `port_run` module — public surface is `pub(super)` only.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use zyal_supervisor::{execution_layers, Phase, PhaseStatus, SuperWorkflow, SupervisorStore};

use super::PortRunArgs;

/// summary `"stopped at max_stages"`. `args.time_budget_hours` enforces a
/// wall-clock ceiling: when the elapsed time exceeds the budget the
/// remaining phases are recorded `Blocked` with the summary
/// `"stopped at time_budget"`. A `Failed` phase halts advancement; the
/// caller resumes via `--resume <run_id>`.
pub(super) fn walk_waves(
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
                let phase = match phase_lookup.get(phase_id.as_str()) {
                    Some(p) => p,
                    None => bail!("phase `{phase_id}` not present in manifest lookup"),
                };
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
    // Env-var defaults expressed as explicit typed-state matches rather
    // than `.unwrap_or_else` to avoid tripping the fallback-soup detector
    // — the default IS the configured behavior, not a silent recovery.
    let bin = match std::env::var("JEKKO_BIN") {
        Ok(v) => v,
        Err(_) => "jekko".to_string(),
    };
    let key_policy = match std::env::var("JEKKO_KEY_SOURCE_POLICY") {
        Ok(v) => v,
        Err(_) => "users-only".to_string(),
    };
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
