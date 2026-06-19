//! `jekko zyal-run --runbook <file.zyal>` — execute a ZYAL runbook directly.
//!
//! This is the missing executor: it parses a `.zyal` runbook (sentinel + YAML
//! body) and *runs* it — a real loop that, each iteration, executes the
//! runbook's `engine.command`, evaluates `stop` / `quality` / `checkpoint.verify`
//! shell asserts, and persists progress to the daemon store (`~/.jekko/jekko.db`)
//! + the NDJSON event ledger (`target/zyal/runs/<id>/events.jsonl`). Because it
//! writes `daemon_run` + `daemon_event` + `daemon_iteration` + reasoning rows,
//! the run is **observable in jekko-web** and `jekko watch <run_id>`.
//!
//! The per-iteration engine command is expected to print a one-line JSON summary
//! on stdout (gen, promotions, best_dE3, …) which is folded into the reasoning
//! graph. This is the "engine is the judge of record; the runbook supervises"
//! model — jekko owns the loop; the engine does one unit of work per tick.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::Args;
use serde::Deserialize;
use serde_json::{json, Value};

use jekko_runner::events::{EventKind, EventSink};
use jekko_store::daemon::{
    self, DaemonEventRow, DaemonIterationRow, DaemonRunRow, ReasoningArtifactRow, ReasoningEdgeRow,
    ReasoningLaneRow,
};
use jekko_store::Db;

use crate::cli::GlobalOpts;

// --------------------------------------------------------------------- runbook
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoopRunbook {
    #[serde(default)]
    pub job: JobBlock,
    #[serde(rename = "loop", default)]
    pub loop_block: LoopCfg,
    #[serde(default)]
    pub engine: EngineBlock,
    #[serde(default)]
    pub stop: StopBlock,
    #[serde(default)]
    pub quality: QualityBlock,
    #[serde(default)]
    pub checkpoint: CheckpointBlock,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct JobBlock {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub objective: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoopCfg {
    #[serde(default = "default_policy")]
    pub policy: String,
    #[serde(default)]
    pub max_iterations: Option<u64>,
    #[serde(default)]
    pub sleep: Option<String>,
    #[serde(default)]
    pub circuit_breaker: Option<CircuitBreaker>,
}

impl Default for LoopCfg {
    fn default() -> Self {
        LoopCfg {
            policy: default_policy(),
            max_iterations: None,
            sleep: None,
            circuit_breaker: None,
        }
    }
}

fn default_policy() -> String {
    "bounded".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CircuitBreaker {
    #[serde(default)]
    pub max_consecutive_errors: Option<u32>,
    #[serde(default)]
    pub on_trip: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EngineBlock {
    /// The shell command run once per iteration. `${RUN_ID}` is substituted.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StopBlock {
    #[serde(default)]
    pub all: Vec<StopItem>,
    #[serde(default)]
    pub any: Vec<StopItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StopItem {
    pub shell: ShellSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShellSpec {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub assert: Option<AssertSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssertSpec {
    #[serde(default)]
    pub exit_code: i32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QualityBlock {
    #[serde(default)]
    pub checks: Vec<QualityCheck>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QualityCheck {
    #[serde(default)]
    pub name: String,
    pub shell: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "default_on_violation")]
    pub on_violation: String,
}

fn default_on_violation() -> String {
    "warn".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CheckpointBlock {
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub verify: Vec<VerifyItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyItem {
    pub command: String,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub assert: Option<AssertSpec>,
}

// ------------------------------------------------------------------------ args
/// `jekko zyal-run` arguments.
#[derive(Args, Debug)]
pub struct ZyalRunArgs {
    /// Path to a `.zyal` runbook (sentinel + YAML body, or plain YAML).
    #[arg(long, value_name = "PATH")]
    pub runbook: PathBuf,

    /// Override the run id. Auto-generated from the job name + timestamp.
    #[arg(long = "run-id", value_name = "ID")]
    pub run_id: Option<String>,

    /// Repo root the run is scoped to (engine cwd + event ledger root).
    /// Defaults to the current directory.
    #[arg(long = "repo-root", value_name = "PATH")]
    pub repo_root: Option<PathBuf>,

    /// Daemon DB path. Defaults to `$JEKKO_DB` or `~/.jekko/jekko.db`.
    #[arg(long, value_name = "PATH")]
    pub db: Option<PathBuf>,

    /// Override the runbook's `loop.max_iterations`.
    #[arg(long = "max-iterations", value_name = "N")]
    pub max_iterations: Option<u64>,
}

const PHASES: &[(&str, &str)] = &[
    ("deep_atlas", "Deep feature atlas"),
    ("science_research", "Science / correlation research"),
    ("ideate", "Ideate (70 workers / 7 users)"),
    ("sandbox_vet", "Sandbox + static vet"),
    ("tiered_validate", "Tiered validation vs E3"),
    ("judge", "Judges: block / pool / improve"),
    ("promote", "Promote (leaderboard)"),
];

// ------------------------------------------------------------------- entry
pub fn run(_global: &GlobalOpts, args: &ZyalRunArgs) -> Result<()> {
    let repo_root = match &args.repo_root {
        Some(p) => p.clone(),
        None => std::env::current_dir().context("resolve cwd")?,
    };
    let repo_root = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);

    let raw = std::fs::read_to_string(&args.runbook)
        .with_context(|| format!("read runbook {}", args.runbook.display()))?;
    let body = zyal_yaml_body(&raw);
    let mut rb: LoopRunbook =
        serde_yaml::from_str(&body).context("parse .zyal runbook (YAML body)")?;
    if let Some(n) = args.max_iterations {
        rb.loop_block.max_iterations = Some(n);
    }
    if rb.engine.command.trim().is_empty() {
        return Err(anyhow!(
            "runbook has no engine.command — nothing to execute per iteration"
        ));
    }

    let run_id = args.run_id.clone().unwrap_or_else(|| {
        let slug = slugify(&rb.job.name);
        format!(
            "{}-{}",
            if slug.is_empty() { "zyal" } else { &slug },
            now_ms()
        )
    });
    let session_id = format!("zyal-{run_id}");

    // Open the daemon DB shared with jekko-web. Disable FK enforcement on THIS
    // connection so we can register a run without materializing a session/project
    // chain — jekko-web reads daemon_run + reasoning rows only, never joins session.
    let db_path = resolve_db_path(args.db.as_deref());
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let db = Db::open(&db_path).with_context(|| format!("open daemon db {}", db_path.display()))?;
    db.connection()
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .ok();
    let conn = db.connection();

    let spec_json = json!({
        "job": { "name": rb.job.name, "objective": rb.job.objective },
        "engine": rb.engine.command,
        "master_plan": {
            "stages": PHASES.iter().enumerate().map(|(i, (id, name))| json!({
                "id": id, "name": name,
                "dependencies": if i == 0 { vec![] } else { vec![PHASES[i - 1].0] },
            })).collect::<Vec<_>>()
        }
    });
    let spec_hash = hash_value(&spec_json);
    let now = now_ms();

    upsert_run_row(
        conn,
        &run_id,
        &session_id,
        "running",
        "deep_atlas",
        &spec_json,
        &spec_hash,
        0,
        now,
        now,
    )?;

    let sink = EventSink::open(&repo_root, &run_id)?;
    let _ = sink.emit(
        EventKind::RunStarted,
        json!({"job": rb.job.name, "runbook": args.runbook.display().to_string()}),
    );
    let mut ev_seq = 0u64;
    record_event(
        conn,
        &run_id,
        0,
        "run_started",
        &json!({"job": rb.job.name}),
        &mut ev_seq,
    )?;
    ensure_lanes(conn, &run_id, now)?;

    let max_iters = rb.loop_block.max_iterations.unwrap_or(20);
    let sleep_secs = rb
        .loop_block
        .sleep
        .as_deref()
        .map(parse_duration_secs)
        .unwrap_or(5);
    let cb_max = rb
        .loop_block
        .circuit_breaker
        .as_ref()
        .and_then(|c| c.max_consecutive_errors)
        .unwrap_or(3);

    println!(
        "[zyal-run] run_id={run_id} db={} policy={} max_iters={} sleep={}s engine=`{}`",
        db_path.display(),
        rb.loop_block.policy,
        max_iters,
        sleep_secs,
        truncate(&rb.engine.command, 80)
    );
    println!("[zyal-run] watch: jekko watch {run_id}   |   web: http://127.0.0.1:8788");

    let mut consecutive_errors = 0u32;
    let mut prev_gen_artifact: Option<String> = None;
    let mut final_status = "complete";
    let mut last_err: Option<String> = None;

    let mut iter: u64 = 0;
    loop {
        // Built-in + runbook stop conditions evaluated BEFORE doing work.
        if iter >= max_iters && rb.loop_block.policy != "forever" {
            break;
        }
        if eval_stop(&rb.stop, &repo_root) {
            println!("[zyal-run] stop condition satisfied; finishing.");
            break;
        }
        iter += 1;

        // --- run the engine command (one unit of work) ---
        let command = rb.engine.command.replace("${RUN_ID}", &run_id);
        let eng_cwd = rb
            .engine
            .cwd
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.clone());
        let started = now_ms();
        let res = run_shell(&command, &eng_cwd, 0); // 0 = no timeout (engine self-bounds)
        let elapsed_s = (now_ms() - started) / 1000;

        let summary = res.as_ref().ok().and_then(|o| parse_summary(&o.stdout));
        let ok = res.as_ref().map(|o| o.code == 0).unwrap_or(false);
        if ok {
            consecutive_errors = 0;
        } else {
            consecutive_errors += 1;
            last_err = Some(match &res {
                Ok(o) => format!("engine exit {} ({})", o.code, truncate(&o.stderr, 160)),
                Err(e) => format!("engine spawn error: {e}"),
            });
            println!(
                "[zyal-run] iter {iter}: {}",
                last_err.as_deref().unwrap_or("")
            );
        }

        // --- persist iteration + event + reasoning graph ---
        let iteration_row = summary.clone().unwrap_or_else(|| json!({}));
        let best = iteration_row.get("best_dE3").and_then(Value::as_f64);
        let promos = iteration_row
            .get("promotions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let gen = iteration_row.get("gen").cloned().unwrap_or(Value::Null);
        let ev_data = json!({
            "iter": iter, "ok": ok, "elapsed_s": elapsed_s,
            "gen": gen, "promotions": promos, "best_dE3": best,
        });
        let _ = sink.emit(EventKind::Heartbeat, compact(&ev_data));
        record_event(
            conn,
            &run_id,
            iter as i64,
            if ok {
                "generation_complete"
            } else {
                "generation_error"
            },
            &ev_data,
            &mut ev_seq,
        )?;
        upsert_iteration_row(
            conn,
            &run_id,
            &session_id,
            iter as i64,
            if ok { "completed" } else { "error" },
            &iteration_row,
            now_ms(),
        )?;

        if let Some(s) = &summary {
            prev_gen_artifact = write_gen_graph(
                conn,
                &run_id,
                iter as i64,
                s,
                prev_gen_artifact.clone(),
                now_ms(),
            )?;
        }

        let phase = if promos > 0 {
            "promote"
        } else {
            "tiered_validate"
        };
        upsert_run_row(
            conn,
            &run_id,
            &session_id,
            "running",
            phase,
            &spec_json,
            &spec_hash,
            iter as i64,
            now,
            now_ms(),
        )?;

        // --- quality checks (shell; on_violation halt|pause|warn) ---
        let mut halt = false;
        for q in &rb.quality.checks {
            if !run_shell_ok(&q.shell, &repo_root) {
                let action = q.on_violation.as_str();
                println!("[zyal-run] quality '{}' violated -> {}", q.name, action);
                record_event(
                    conn,
                    &run_id,
                    iter as i64,
                    "quality_violation",
                    &json!({"check": q.name, "action": action}),
                    &mut ev_seq,
                )?;
                match action {
                    "halt" => {
                        halt = true;
                        final_status = "failed";
                        last_err = Some(format!("quality halt: {}", q.name));
                    }
                    "pause" => {
                        final_status = "paused";
                    }
                    _ => {}
                }
                if halt {
                    break;
                }
            }
        }
        if halt {
            break;
        }

        // --- checkpoint.verify (shell asserts; informational — COXMAZE self-commits nothing) ---
        for v in &rb.checkpoint.verify {
            let spec = ShellSpec {
                command: v.command.clone(),
                cwd: None,
                timeout: v.timeout.clone(),
                assert: v.assert.clone(),
            };
            let pass = shell_assert_passes(&spec, &repo_root);
            if !pass {
                record_event(
                    conn,
                    &run_id,
                    iter as i64,
                    "checkpoint_verify_failed",
                    &json!({"command": truncate(&v.command, 80)}),
                    &mut ev_seq,
                )?;
            }
        }

        // --- circuit breaker ---
        if consecutive_errors >= cb_max {
            println!("[zyal-run] circuit breaker tripped ({consecutive_errors} consecutive errors); pausing.");
            final_status = "failed";
            break;
        }

        // engine may signal terminal completion via summary.done == true
        if summary
            .as_ref()
            .and_then(|s| s.get("done"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            println!("[zyal-run] engine reported done; finishing.");
            break;
        }

        if iter < max_iters && sleep_secs > 0 {
            std::thread::sleep(Duration::from_secs(sleep_secs));
        }
    }

    let fin = now_ms();
    upsert_run_with_stop(
        conn,
        &run_id,
        &session_id,
        final_status,
        "promote",
        &spec_json,
        &spec_hash,
        iter as i64,
        now,
        fin,
        last_err.as_deref(),
    )?;
    let _ = sink.emit(
        EventKind::RunFinished,
        json!({"status": final_status, "iterations": iter}),
    );
    record_event(
        conn,
        &run_id,
        iter as i64,
        "run_finished",
        &json!({"status": final_status, "iterations": iter}),
        &mut ev_seq,
    )?;
    println!("[zyal-run] DONE status={final_status} iterations={iter} run_id={run_id}");
    Ok(())
}

// ------------------------------------------------------------------- db helpers
#[allow(clippy::too_many_arguments)]
fn upsert_run_row(
    conn: &rusqlite::Connection,
    run_id: &str,
    session_id: &str,
    status: &str,
    phase: &str,
    spec: &Value,
    spec_hash: &str,
    iteration: i64,
    created: i64,
    updated: i64,
) -> Result<()> {
    daemon::upsert_run(
        conn,
        &DaemonRunRow {
            id: run_id.to_string(),
            root_session_id: session_id.to_string(),
            active_session_id: session_id.to_string(),
            status: status.to_string(),
            phase: phase.to_string(),
            spec_json: spec.clone(),
            spec_hash: spec_hash.to_string(),
            iteration,
            epoch: 0,
            last_error: None,
            last_exit_result_json: None,
            stopped_at: None,
            time_created: created,
            time_updated: updated,
        },
    )
    .map_err(|e| anyhow!("upsert_run: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn upsert_run_with_stop(
    conn: &rusqlite::Connection,
    run_id: &str,
    session_id: &str,
    status: &str,
    phase: &str,
    spec: &Value,
    spec_hash: &str,
    iteration: i64,
    created: i64,
    stopped: i64,
    err: Option<&str>,
) -> Result<()> {
    daemon::upsert_run(
        conn,
        &DaemonRunRow {
            id: run_id.to_string(),
            root_session_id: session_id.to_string(),
            active_session_id: session_id.to_string(),
            status: status.to_string(),
            phase: phase.to_string(),
            spec_json: spec.clone(),
            spec_hash: spec_hash.to_string(),
            iteration,
            epoch: 0,
            last_error: err.map(|s| s.to_string()),
            last_exit_result_json: None,
            stopped_at: Some(stopped),
            time_created: created,
            time_updated: stopped,
        },
    )
    .map_err(|e| anyhow!("upsert_run(stop): {e}"))
}

fn record_event(
    conn: &rusqlite::Connection,
    run_id: &str,
    iteration: i64,
    kind: &str,
    data: &Value,
    seq: &mut u64,
) -> Result<()> {
    *seq += 1;
    let now = now_ms();
    daemon::insert_event(
        conn,
        &DaemonEventRow {
            id: format!("{run_id}-ev-{seq}"),
            run_id: run_id.to_string(),
            iteration,
            event_type: kind.to_string(),
            payload_json: data.clone(),
            time_created: now,
            time_updated: now,
        },
    )
    .map_err(|e| anyhow!("insert_event: {e}"))
}

fn upsert_iteration_row(
    conn: &rusqlite::Connection,
    run_id: &str,
    session_id: &str,
    iteration: i64,
    reason: &str,
    result: &Value,
    now: i64,
) -> Result<()> {
    daemon::upsert_iteration(
        conn,
        &DaemonIterationRow {
            run_id: run_id.to_string(),
            iteration,
            session_id: session_id.to_string(),
            terminal_reason: reason.to_string(),
            result_json: result.clone(),
            token_usage_json: None,
            cost: None,
            checkpoint_sha: None,
            time_created: now,
            time_updated: now,
        },
    )
    .map_err(|e| anyhow!("upsert_iteration: {e}"))
}

const LANES: &[(&str, &str)] = &[
    ("experiment", "license-clean experiment architect"),
    ("stage", "stage inventor (sandboxed)"),
    ("feature", "feature analyst"),
    ("research", "science researcher (cited)"),
];

fn ensure_lanes(conn: &rusqlite::Connection, run_id: &str, now: i64) -> Result<()> {
    for (role, strat) in LANES {
        daemon::upsert_reasoning_lane(
            conn,
            &ReasoningLaneRow {
                id: format!("{run_id}-lane-{role}"),
                run_id: run_id.to_string(),
                role: role.to_string(),
                strategy: strat.to_string(),
                status: "running".to_string(),
                artifact_ids: vec![],
                write_scope: vec![],
                worker_id: None,
                confidence: 0.0,
                time_created: now,
                time_updated: now,
            },
        )
        .map_err(|e| anyhow!("upsert_lane: {e}"))?;
    }
    Ok(())
}

/// Write a per-generation artifact + promotion artifacts + edges. Returns the new gen artifact id.
fn write_gen_graph(
    conn: &rusqlite::Connection,
    run_id: &str,
    gen: i64,
    s: &Value,
    prev: Option<String>,
    now: i64,
) -> Result<Option<String>> {
    let promos = s.get("promotions").and_then(|v| v.as_i64()).unwrap_or(0);
    let best = s.get("best_dE3").and_then(|v| v.as_f64());
    let gen_id = format!("{run_id}-gen-{gen}");
    let title = format!(
        "gen {gen} • +{promos} promo • best ΔE3 {}",
        best.map(|b| format!("{b:.4}"))
            .unwrap_or_else(|| "n/a".into())
    );
    let conf = best
        .map(|b| (0.5 + b * 40.0).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    daemon::upsert_reasoning_artifact(
        conn,
        &ReasoningArtifactRow {
            id: gen_id.clone(),
            run_id: run_id.to_string(),
            role: "experiment".to_string(),
            kind: "generation".to_string(),
            title,
            summary: compact(s).to_string(),
            evidence_level: "validated".to_string(),
            confidence: conf,
            payload_json: Some(compact(s)),
            content_hash: hash_value(s),
            status: "candidate".to_string(),
            time_created: now,
            time_updated: now,
        },
    )
    .map_err(|e| anyhow!("upsert_artifact(gen): {e}"))?;
    if let Some(p) = prev {
        daemon::upsert_reasoning_edge(
            conn,
            &ReasoningEdgeRow {
                run_id: run_id.to_string(),
                src_artifact_id: p,
                dst_artifact_id: gen_id.clone(),
                kind: "feeds".to_string(),
                weight: Some(1.0),
                payload_json: None,
                time_created: now,
            },
        )
        .map_err(|e| anyhow!("upsert_edge(feeds): {e}"))?;
    }
    if let Some(tops) = s.get("top_promotions").and_then(|v| v.as_array()) {
        for (i, t) in tops.iter().take(5).enumerate() {
            let eid = t.get("eid").and_then(|v| v.as_str()).unwrap_or("cox");
            let d = t.get("dE3").and_then(|v| v.as_f64());
            let pid = format!("{run_id}-promo-{gen}-{i}");
            daemon::upsert_reasoning_artifact(
                conn,
                &ReasoningArtifactRow {
                    id: pid.clone(),
                    run_id: run_id.to_string(),
                    role: "experiment".to_string(),
                    kind: "promotion".to_string(),
                    title: format!(
                        "{eid} ΔE3={}",
                        d.map(|x| format!("{x:+.4}")).unwrap_or_default()
                    ),
                    summary: String::new(),
                    evidence_level: "validated".to_string(),
                    confidence: d.map(|b| (0.5 + b * 40.0).clamp(0.0, 1.0)).unwrap_or(0.6),
                    payload_json: None,
                    content_hash: pid.clone(),
                    status: "promoted".to_string(),
                    time_created: now,
                    time_updated: now,
                },
            )
            .map_err(|e| anyhow!("upsert_artifact(promo): {e}"))?;
            daemon::upsert_reasoning_edge(
                conn,
                &ReasoningEdgeRow {
                    run_id: run_id.to_string(),
                    src_artifact_id: gen_id.clone(),
                    dst_artifact_id: pid,
                    kind: "refines".to_string(),
                    weight: Some(1.0),
                    payload_json: None,
                    time_created: now,
                },
            )
            .map_err(|e| anyhow!("upsert_edge(refines): {e}"))?;
        }
    }
    Ok(Some(gen_id))
}

// ------------------------------------------------------------------- shell helpers
struct ShellOut {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run a shell command (`sh -c`), optionally killed after `timeout_secs` (0 = none).
fn run_shell(command: &str, cwd: &Path, timeout_secs: u64) -> Result<ShellOut> {
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn sh -c {}", truncate(command, 80)))?;
    let pid = child.id();
    if timeout_secs == 0 {
        let out = child.wait_with_output().context("wait engine")?;
        return Ok(ShellOut {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Ok(out)) => Ok(ShellOut {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }),
        Ok(Err(e)) => Err(anyhow!("shell wait error: {e}")),
        Err(_) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            Ok(ShellOut {
                code: -1,
                stdout: String::new(),
                stderr: format!("timed out after {timeout_secs}s"),
            })
        }
    }
}

fn shell_assert_passes(spec: &ShellSpec, repo_root: &Path) -> bool {
    let cwd = spec
        .cwd
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.to_path_buf());
    let timeout = spec
        .timeout
        .as_deref()
        .map(parse_duration_secs)
        .unwrap_or(60);
    let want = spec.assert.as_ref().map(|a| a.exit_code).unwrap_or(0);
    match run_shell(&spec.command, &cwd, timeout) {
        Ok(o) => o.code == want,
        Err(_) => false,
    }
}

fn run_shell_ok(command: &str, repo_root: &Path) -> bool {
    matches!(run_shell(command, repo_root, 120), Ok(o) if o.code == 0)
}

/// `stop.all` satisfied = every assert passes; `stop.any` satisfied = any passes.
fn eval_stop(stop: &StopBlock, repo_root: &Path) -> bool {
    let all_ok = !stop.all.is_empty()
        && stop
            .all
            .iter()
            .all(|it| shell_assert_passes(&it.shell, repo_root));
    let any_ok = stop
        .any
        .iter()
        .any(|it| shell_assert_passes(&it.shell, repo_root));
    all_ok || any_ok
}

// ------------------------------------------------------------------- misc helpers
/// Strip the `<<<ZYAL …>>> … <<<END_ZYAL>>>` sentinels, returning the YAML body.
fn zyal_yaml_body(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    });
    let Some(idx) = first else {
        return text.to_string();
    };
    if !lines[idx].starts_with("<<<ZYAL ") {
        return text.to_string();
    }
    let mut body = Vec::new();
    for line in lines.into_iter().skip(idx + 1) {
        if line.starts_with("<<<END_ZYAL ") {
            return body.join("\n");
        }
        body.push(line);
    }
    text.to_string()
}

fn parse_summary(stdout: &str) -> Option<Value> {
    for line in stdout.lines().rev() {
        let t = line.trim();
        if t.starts_with('{') && t.ends_with('}') {
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(t) {
                return Some(v);
            }
        }
    }
    None
}

fn resolve_db_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    if let Some(p) = std::env::var_os("JEKKO_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".jekko").join("jekko.db")
}

fn parse_duration_secs(s: &str) -> u64 {
    let s = s.trim();
    let (num, mult) = if let Some(v) = s.strip_suffix('h') {
        (v, 3600)
    } else if let Some(v) = s.strip_suffix('m') {
        (v, 60)
    } else if let Some(v) = s.strip_suffix('s') {
        (v, 1)
    } else {
        (s, 1)
    };
    num.trim()
        .parse::<f64>()
        .map(|n| (n * mult as f64) as u64)
        .unwrap_or(5)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn hash_value(v: &Value) -> String {
    let mut h = DefaultHasher::new();
    v.to_string().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= n {
        s
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// Keep an event payload comfortably under the 512-byte NDJSON line cap.
fn compact(v: &Value) -> Value {
    let s = v.to_string();
    if s.len() <= 360 {
        v.clone()
    } else {
        json!({ "summary": truncate(&s, 300) })
    }
}
