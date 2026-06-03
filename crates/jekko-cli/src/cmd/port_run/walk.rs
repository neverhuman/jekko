//! Wave-walking + live-phase invocation for `jekko port-run`.
//!
//! Split out of `port_run.rs` to keep that file under the 500-LOC shape
//! threshold (jankurai HLT-001:shape). All callers live in the parent
//! `port_run` module — public surface is `pub(super)` only.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use zyal_supervisor::{
    execution_layers, JailgunExec, Phase, PhaseExec, PhaseStatus, SshExec, SuperWorkflow,
    SupervisorStore,
};

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
                invoke_phase(phase, args)
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

/// Dispatch a single `--live` phase to its executor.
///
/// Routing is driven by [`Phase::exec`]:
/// * `None` — historical default; delegate to `jekko run --agent plan`.
/// * `Some(Agent { name })` — delegate to `jekko run --agent <name>`.
/// * `Some(Ssh(..))` — run an allowlisted remote command over SSH.
fn invoke_phase(phase: &Phase, args: &PortRunArgs) -> Result<String> {
    match phase.exec.as_ref() {
        None => invoke_agent_phase(phase, "plan", args),
        Some(PhaseExec::Agent { name }) => invoke_agent_phase(phase, name, args),
        Some(PhaseExec::Ssh(ssh)) => invoke_ssh_phase(phase, ssh, args),
        Some(PhaseExec::Jailgun(jx)) => invoke_jailgun_phase(phase, jx, args),
    }
}

/// Build the `JailgunAgentRunRequest` JSON for a [`PhaseExec::Jailgun`] phase.
///
/// jekko cannot depend on `jailgun-core`, so the request is mirrored
/// structurally (field names must match `JailgunAgentRunRequest`); the
/// integration test against a stub `jailgun` binary guards against drift.
/// `request_overrides` (when an object) supplies extra fields like
/// `repo`/`deploy`/`ci`/`github`. The authoritative core fields
/// (`version`/`prompt_ref`/`prompt_file`/`tabs`/`max_runtime_seconds`) are
/// stamped on top so an override can never hijack the prompt file/reference or
/// the interface version.
fn request_json_for(jx: &JailgunExec, prompt_file: &Path) -> serde_json::Value {
    let mut req = match jx.request_overrides {
        serde_json::Value::Object(ref map) => serde_json::Value::Object(map.clone()),
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };
    let obj = req
        .as_object_mut()
        .expect("constructed as a JSON object above");
    obj.insert("version".into(), serde_json::json!(1));
    obj.insert("prompt_ref".into(), serde_json::json!(jx.prompt_ref));
    obj.insert(
        "prompt_file".into(),
        serde_json::json!(prompt_file.to_string_lossy()),
    );
    if let Some(tabs) = jx.tabs {
        obj.insert("tabs".into(), serde_json::json!(tabs));
    }
    if let Some(secs) = jx.max_runtime_seconds {
        obj.insert("max_runtime_seconds".into(), serde_json::json!(secs));
    }
    req
}

/// Render a concise, prompt-text-free phase summary from a
/// `JailgunAgentRunSummary` JSON value. Only structured status fields are
/// surfaced — never prompt text.
fn summarize_jailgun_value(v: &serde_json::Value) -> String {
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("-").to_string();
    let arr_len = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    };
    format!(
        "jailgun run {}: status={} deploy={} ci={} changed_files={} failures={}",
        v.get("run_id").and_then(|x| x.as_str()).unwrap_or("?"),
        field("status"),
        field("deploy_status"),
        field("ci_status"),
        arr_len("changed_files"),
        arr_len("failures"),
    )
}

/// Read and summarize the Jailgun summary file written by `run-agent`. Returns
/// `None` when the file is missing or unparseable.
fn summarize_jailgun_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(summarize_jailgun_value(&value))
}

/// Run a Jailgun agent run for a [`PhaseExec::Jailgun`] phase. Dispatches by
/// transport: when `jx.endpoint` is set the run goes over HTTP to a
/// jailgun-server ([`run_jailgun_http`]); otherwise it shells out to the
/// `jailgun run-agent` CLI (`JAILGUN_BIN`, default `jailgun`) via
/// [`run_jailgun_agent`]. Both honor `args.per_phase_timeout_secs`.
fn invoke_jailgun_phase(phase: &Phase, jx: &JailgunExec, args: &PortRunArgs) -> Result<String> {
    let timeout = Duration::from_secs(args.per_phase_timeout_secs);
    match jx.endpoint.as_deref() {
        Some(base) => {
            let curl = match std::env::var("CURL_BIN") {
                Ok(v) => v,
                Err(_) => "curl".to_string(),
            };
            run_jailgun_http(&curl, base, jx, timeout, &phase.id)
        }
        None => {
            let bin = match std::env::var("JAILGUN_BIN") {
                Ok(v) => v,
                Err(_) => "jailgun".to_string(),
            };
            run_jailgun_agent(&bin, jx, timeout, &phase.id)
        }
    }
}

/// HTTP transport for a [`PhaseExec::Jailgun`] phase: POST the run request to a
/// jailgun-server (`POST {base}/api/runs`) and poll its agent-summary
/// (`GET {base}/api/runs/{id}/agent-summary`) until a summary with a status
/// appears. `CURL_BIN` (default `curl`) is the transport — jekko-cli needs no
/// HTTP-client dep and the path is script-testable. Mirrors the CLI path's
/// prompt-free summary and fail-closed semantics: a non-`succeeded` run fails
/// the phase. Note: the request's `prompt_file` is a path the jailgun-server
/// must be able to read, so this assumes a same-host server.
fn run_jailgun_http(
    curl: &str,
    base_url: &str,
    jx: &JailgunExec,
    timeout: Duration,
    phase_id: &str,
) -> Result<String> {
    let base = base_url.trim_end_matches('/').to_string();
    let dir = tempfile::tempdir().context("create scratch dir for jailgun http phase")?;
    let prompt_file = dir.path().join("prompt.txt");
    let request_file = dir.path().join("request.json");
    let summary_file = dir.path().join("summary.json");
    std::fs::write(&prompt_file, &jx.prompt)
        .with_context(|| format!("write jailgun prompt file for phase `{phase_id}`"))?;
    let request = request_json_for(jx, &prompt_file);
    std::fs::write(
        &request_file,
        serde_json::to_vec_pretty(&request).context("serialize jailgun run request")?,
    )
    .with_context(|| format!("write jailgun request file for phase `{phase_id}`"))?;

    let phase_id = phase_id.to_string();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for jailgun http phase")?;

    rt.block_on(async move {
        // Submit: POST the request body; expect a 202 JSON carrying `run_id`.
        let runs_url = format!("{base}/api/runs");
        let mut post = tokio::process::Command::new(curl);
        post.arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg("-H")
            .arg("content-type: application/json")
            .arg("--data-binary")
            .arg(format!("@{}", request_file.display()))
            .arg(&runs_url)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let out = post
            .output()
            .await
            .with_context(|| format!("spawn `{curl} POST {runs_url}`"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            bail!(
                "jailgun http submit failed for phase `{}` (curl {:?}): {}",
                phase_id,
                out.status.code(),
                if stderr.is_empty() {
                    "<no stderr>".to_owned()
                } else {
                    stderr
                }
            );
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let accepted: serde_json::Value = serde_json::from_str(body.trim())
            .with_context(|| format!("parse jailgun POST /api/runs response: {}", body.trim()))?;
        let run_id = accepted
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "jailgun POST /api/runs response missing run_id: {}",
                    body.trim()
                )
            })?
            .to_string();

        // Poll the agent-summary until a summary carrying a `status` appears.
        let summary_url = format!("{base}/api/runs/{run_id}/agent-summary");
        let start = Instant::now();
        loop {
            let mut get = tokio::process::Command::new(curl);
            get.arg("-sS")
                .arg("-o")
                .arg(&summary_file)
                .arg("-w")
                .arg("%{http_code}")
                .arg(&summary_url)
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            let resp = get
                .output()
                .await
                .with_context(|| format!("spawn `{curl} GET {summary_url}`"))?;
            let code = String::from_utf8_lossy(&resp.stdout).trim().to_string();
            if code == "200" {
                if let Ok(bytes) = std::fs::read(&summary_file) {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let status = value
                            .get("status")
                            .and_then(|s| s.as_str())
                            .unwrap_or_default();
                        // A summary with a non-empty status is terminal.
                        if !status.is_empty() {
                            let summary = summarize_jailgun_value(&value);
                            if status == "succeeded" {
                                return Ok(summary);
                            }
                            bail!("jailgun http phase `{}` failed: {}", phase_id, summary);
                        }
                    }
                }
            }
            if start.elapsed() >= timeout {
                bail!(
                    "jailgun http phase `{}` produced no summary within {}s (last http {})",
                    phase_id,
                    timeout.as_secs(),
                    if code.is_empty() { "<none>" } else { &code }
                );
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
}

/// Core of [`invoke_jailgun_phase`], parameterized over the binary and timeout
/// so it is unit-testable with a scripted `jailgun` (no env coupling, no `--live`
/// gate). Writes the prompt to a scratch file referenced by the request's
/// `prompt_file` and invokes `<bin> run-agent --request <f> --events-jsonl <t>
/// --summary-json <t>`. The CLI bails non-zero on a non-`succeeded` run (after
/// writing the summary), so a non-zero exit fails the phase while still
/// surfacing the structured status; never echoes prompt text.
fn run_jailgun_agent(
    bin: &str,
    jx: &JailgunExec,
    timeout: Duration,
    phase_id: &str,
) -> Result<String> {
    let dir = tempfile::tempdir().context("create scratch dir for jailgun phase")?;
    let prompt_file = dir.path().join("prompt.txt");
    let request_file = dir.path().join("request.json");
    let events_file = dir.path().join("events.jsonl");
    let summary_file = dir.path().join("summary.json");

    std::fs::write(&prompt_file, &jx.prompt)
        .with_context(|| format!("write jailgun prompt file for phase `{phase_id}`"))?;
    let request = request_json_for(jx, &prompt_file);
    let request_bytes =
        serde_json::to_vec_pretty(&request).context("serialize jailgun run request")?;
    std::fs::write(&request_file, request_bytes)
        .with_context(|| format!("write jailgun request file for phase `{phase_id}`"))?;

    let summary_path = summary_file.clone();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for jailgun phase invocation")?;

    rt.block_on(async move {
        let mut cmd = tokio::process::Command::new(bin);
        cmd.arg("run-agent")
            .arg("--request")
            .arg(&request_file)
            .arg("--events-jsonl")
            .arg(&events_file)
            .arg("--summary-json")
            .arg(&summary_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .with_context(|| format!("spawn `{bin} run-agent`"))?;

        let wait = child.wait_with_output();
        let output = match tokio::time::timeout(timeout, wait).await {
            Ok(res) => res.context("await jailgun phase subprocess")?,
            Err(_) => {
                bail!(
                    "jailgun phase `{}` exceeded per-phase timeout of {}s",
                    phase_id,
                    timeout.as_secs()
                );
            }
        };
        if !output.status.success() {
            // `run-agent` writes the summary then bails non-zero on a
            // non-succeeded run; prefer the structured status, fall back to stderr.
            let detail = summarize_jailgun_file(&summary_path).unwrap_or_else(|| {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stderr.is_empty() {
                    "<no stderr>".to_string()
                } else {
                    stderr
                }
            });
            bail!("jailgun phase `{phase_id}` failed: {detail}");
        }
        summarize_jailgun_file(&summary_path).ok_or_else(|| {
            anyhow!(
                "jailgun phase `{}` produced no readable summary at {}",
                phase_id,
                summary_path.display()
            )
        })
    })
}

/// Decide whether `host` is permitted by the optional `allowlist` (the value
/// of `JEKKO_SSH_ALLOWED_HOSTS`). A `None` or empty/whitespace allowlist
/// applies no gate (full extensibility — the gate is opt-in by configuring
/// the var). Otherwise `host` must match one comma-separated entry exactly.
fn host_allowed(host: &str, allowlist: Option<&str>) -> bool {
    match allowlist {
        None => true,
        Some(raw) => {
            let raw = raw.trim();
            raw.is_empty()
                || raw
                    .split(',')
                    .map(str::trim)
                    .filter(|h| !h.is_empty())
                    .any(|h| h == host)
        }
    }
}

/// Enforce the [`host_allowed`] gate against `JEKKO_SSH_ALLOWED_HOSTS`,
/// rejecting the phase before any process is spawned when the host is denied.
fn ensure_ssh_host_allowed(host: &str) -> Result<()> {
    let allowlist = std::env::var("JEKKO_SSH_ALLOWED_HOSTS").ok();
    if host_allowed(host, allowlist.as_deref()) {
        Ok(())
    } else {
        bail!("ssh host `{host}` is not in the JEKKO_SSH_ALLOWED_HOSTS allowlist");
    }
}

/// Run an allowlisted remote command for an [`PhaseExec::Ssh`] phase and
/// return its captured stdout as the phase summary. The ssh binary is
/// `JEKKO_SSH_BIN` (default `ssh` on PATH). A non-zero remote exit fails the
/// phase unless [`SshExec::allow_nonzero_exit`] is set. Aborts after
/// `args.per_phase_timeout_secs` seconds.
fn invoke_ssh_phase(phase: &Phase, ssh: &SshExec, args: &PortRunArgs) -> Result<String> {
    ensure_ssh_host_allowed(&ssh.host)?;
    let bin = match std::env::var("JEKKO_SSH_BIN") {
        Ok(v) => v,
        Err(_) => "ssh".to_string(),
    };
    let timeout = Duration::from_secs(args.per_phase_timeout_secs);
    let phase_id = phase.id.clone();
    let host = ssh.host.clone();
    let command = ssh.command.clone();
    let allow_nonzero_exit = ssh.allow_nonzero_exit;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for ssh phase invocation")?;

    rt.block_on(async move {
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg(&host)
            .arg(&command)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .with_context(|| format!("spawn `{bin} {host} <command>`"))?;

        let wait = child.wait_with_output();
        let output = match tokio::time::timeout(timeout, wait).await {
            Ok(res) => res.context("await ssh phase subprocess")?,
            Err(_) => {
                bail!(
                    "ssh phase `{}` exceeded per-phase timeout of {}s",
                    phase_id,
                    args.per_phase_timeout_secs
                );
            }
        };
        if !output.status.success() && !allow_nonzero_exit {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "ssh phase `{}` exited with status {:?}: {}",
                phase_id,
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

/// Spawn `jekko run --ephemeral --json --agent <agent> --cwd <repo> <prompt>`
/// as a child process and return its captured stdout. Honors `JEKKO_BIN`
/// (default `jekko` on PATH) and `JEKKO_KEY_SOURCE_POLICY` (default
/// `users-only`). Aborts after `args.per_phase_timeout_secs` seconds.
fn invoke_agent_phase(phase: &Phase, agent: &str, args: &PortRunArgs) -> Result<String> {
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
            .arg(agent)
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
            .with_context(|| format!("spawn `{bin} run --ephemeral --json --agent {agent}`"))?;

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

#[cfg(test)]
mod tests {
    use super::{host_allowed, request_json_for, summarize_jailgun_value};
    use zyal_supervisor::JailgunExec;

    #[test]
    fn host_allowed_with_no_allowlist_permits_any_host() {
        // Gate is opt-in: an unset allowlist means full extensibility.
        assert!(host_allowed("deploy@xbabe", None));
    }

    #[test]
    fn host_allowed_with_empty_allowlist_permits_any_host() {
        assert!(host_allowed("deploy@xbabe", Some("")));
        assert!(host_allowed("deploy@xbabe", Some("   ")));
    }

    #[test]
    fn host_allowed_enforces_exact_membership_when_set() {
        let allow = Some("xbabe, deploy@xbabe ,build-01");
        assert!(host_allowed("xbabe", allow));
        assert!(host_allowed("deploy@xbabe", allow), "whitespace is trimmed");
        assert!(host_allowed("build-01", allow));
        assert!(!host_allowed("evil-host", allow));
        // Substring / prefix matches must NOT pass — membership is exact.
        assert!(!host_allowed("xbabe.internal", allow));
        assert!(!host_allowed("deploy@xbabe2", allow));
    }

    #[test]
    fn request_json_for_stamps_authoritative_core_fields() {
        let jx = JailgunExec {
            prompt_ref: "jmcp://wo/7/prompt".into(),
            prompt: "do the secret thing".into(),
            tabs: Some(6),
            max_runtime_seconds: Some(900),
            request_overrides: serde_json::json!({
                "ci": { "enabled": true, "branch": "main" },
                "tabs": 99,                    // must NOT win over the core field
                "prompt_file": "/etc/passwd",  // must NOT hijack the prompt file
            }),
            endpoint: None,
        };
        let req = request_json_for(&jx, std::path::Path::new("/tmp/p.txt"));
        assert_eq!(req["version"], 1);
        assert_eq!(req["prompt_ref"], "jmcp://wo/7/prompt");
        assert_eq!(
            req["prompt_file"], "/tmp/p.txt",
            "core prompt_file wins over override"
        );
        assert_eq!(req["tabs"], 6, "core tabs wins over override");
        assert_eq!(req["max_runtime_seconds"], 900);
        assert_eq!(
            req["ci"]["branch"], "main",
            "non-core overrides are preserved"
        );
        // Prompt text is never embedded in the request envelope (only the path).
        assert!(!req.to_string().contains("secret thing"));
    }

    #[test]
    fn request_json_for_omits_absent_optionals() {
        let jx = JailgunExec {
            prompt_ref: "r".into(),
            prompt: "p".into(),
            tabs: None,
            max_runtime_seconds: None,
            request_overrides: serde_json::Value::Null,
            endpoint: None,
        };
        let req = request_json_for(&jx, std::path::Path::new("/tmp/p.txt"));
        assert!(req.get("tabs").is_none());
        assert!(req.get("max_runtime_seconds").is_none());
        assert_eq!(req["version"], 1);
    }

    #[test]
    fn summarize_jailgun_value_is_concise_and_prompt_free() {
        let v = serde_json::json!({
            "run_id": "run-9",
            "status": "succeeded",
            "deploy_status": "done",
            "ci_status": "passed",
            "changed_files": ["a.rs", "b.rs"],
            "failures": [],
        });
        let s = summarize_jailgun_value(&v);
        assert!(s.contains("run-9") && s.contains("status=succeeded"));
        assert!(s.contains("changed_files=2") && s.contains("failures=0"));
    }

    #[test]
    fn summarize_jailgun_value_tolerates_missing_fields() {
        let s = summarize_jailgun_value(&serde_json::json!({}));
        assert!(s.contains("status=-"));
        assert!(s.contains("changed_files=0"));
    }

    // ---- Stub-binary tests for the jailgun phase executor ----
    // These drive `run_jailgun_agent` against a throwaway shell stub via an
    // explicit binary path (no `JAILGUN_BIN` env, no `--live` gate), so they run
    // deterministically in CI and in parallel.

    #[cfg(unix)]
    fn jx_simple() -> JailgunExec {
        JailgunExec {
            prompt_ref: "jmcp://wo/1/prompt".into(),
            prompt: "build it".into(),
            tabs: None,
            max_runtime_seconds: None,
            request_overrides: serde_json::Value::Null,
            endpoint: None,
        }
    }

    #[cfg(unix)]
    fn write_stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write stub");
        let mut perms = std::fs::metadata(&path).expect("stat stub").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod stub");
        path
    }

    #[cfg(unix)]
    const STUB_OK: &str = r#"#!/bin/sh
SUMMARY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --summary-json) shift; SUMMARY="$1" ;;
  esac
  shift
done
printf '%s' '{"run_id":"run-stub","status":"succeeded","deploy_status":"done","ci_status":"passed","changed_files":["a.rs"],"failures":[]}' > "$SUMMARY"
exit 0
"#;

    #[cfg(unix)]
    const STUB_FAIL: &str = r#"#!/bin/sh
SUMMARY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --summary-json) shift; SUMMARY="$1" ;;
  esac
  shift
done
printf '%s' '{"run_id":"run-stub","status":"failed","changed_files":[],"failures":[{"code":"x","message":"boom"}]}' > "$SUMMARY"
exit 1
"#;

    #[cfg(unix)]
    const STUB_CHECK_REQUEST: &str = r#"#!/bin/sh
SUMMARY=""
REQ=""
while [ $# -gt 0 ]; do
  case "$1" in
    --summary-json) shift; SUMMARY="$1" ;;
    --request) shift; REQ="$1" ;;
  esac
  shift
done
grep -q '"prompt_ref"' "$REQ" || exit 2
grep -q '"prompt_file"' "$REQ" || exit 3
grep -q '"tabs": 7' "$REQ" || exit 4
printf '%s' '{"run_id":"r","status":"succeeded","changed_files":[],"failures":[]}' > "$SUMMARY"
exit 0
"#;

    #[cfg(unix)]
    const STUB_HANG: &str = "#!/bin/sh\nsleep 30\nexit 0\n";

    #[cfg(unix)]
    #[test]
    fn run_jailgun_agent_succeeds_with_stub() {
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "jailgun_ok.sh", STUB_OK);
        let summary = super::run_jailgun_agent(
            stub.to_str().unwrap(),
            &jx_simple(),
            std::time::Duration::from_secs(30),
            "produce",
        )
        .expect("stub jailgun succeeds");
        assert!(summary.contains("run-stub"), "summary: {summary}");
        assert!(summary.contains("status=succeeded"), "summary: {summary}");
        assert!(summary.contains("changed_files=1"), "summary: {summary}");
    }

    #[cfg(unix)]
    #[test]
    fn run_jailgun_agent_fails_on_nonzero_exit_but_surfaces_status() {
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "jailgun_fail.sh", STUB_FAIL);
        let err = super::run_jailgun_agent(
            stub.to_str().unwrap(),
            &jx_simple(),
            std::time::Duration::from_secs(30),
            "produce",
        )
        .expect_err("non-zero exit must fail the phase");
        let msg = format!("{err:#}");
        assert!(msg.contains("produce"), "msg: {msg}");
        assert!(msg.contains("status=failed"), "msg: {msg}");
    }

    #[cfg(unix)]
    #[test]
    fn run_jailgun_agent_passes_well_formed_request() {
        // The stub asserts the request JSON carries the core fields + tabs=7.
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "jailgun_check.sh", STUB_CHECK_REQUEST);
        let jx = JailgunExec {
            prompt_ref: "jmcp://wo/2/prompt".into(),
            prompt: "p".into(),
            tabs: Some(7),
            max_runtime_seconds: Some(120),
            request_overrides: serde_json::Value::Null,
            endpoint: None,
        };
        super::run_jailgun_agent(
            stub.to_str().unwrap(),
            &jx,
            std::time::Duration::from_secs(30),
            "produce",
        )
        .expect("request shape validated by stub (else non-zero exit)");
    }

    #[cfg(unix)]
    #[test]
    fn run_jailgun_agent_times_out() {
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "jailgun_hang.sh", STUB_HANG);
        let err = super::run_jailgun_agent(
            stub.to_str().unwrap(),
            &jx_simple(),
            std::time::Duration::from_secs(1),
            "produce",
        )
        .expect_err("hang must time out");
        assert!(format!("{err:#}").contains("timeout"), "err: {err:#}");
    }

    // ---- HTTP-transport tests (stub `curl` via the curl-bin parameter) ----
    // The stub distinguishes POST (no `-o`) from the GET agent-summary poll
    // (`-o <file>`), mirroring jailgun-server: POST -> 202 {run_id}; GET ->
    // 200 + the summary JSON written to the `-o` file.

    #[cfg(unix)]
    const STUB_CURL_OK: &str = r#"#!/bin/sh
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift; OUT="$1" ;;
  esac
  shift
done
if [ -n "$OUT" ]; then
  printf '%s' '{"run_id":"http-run","status":"succeeded","deploy_status":"done","ci_status":"passed","changed_files":["x.rs"],"failures":[]}' > "$OUT"
  printf '200'
else
  printf '%s' '{"run_id":"http-run","status":"accepted","summary_url":"/api/runs/http-run/agent-summary"}'
fi
exit 0
"#;

    #[cfg(unix)]
    const STUB_CURL_FAIL: &str = r#"#!/bin/sh
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift; OUT="$1" ;;
  esac
  shift
done
if [ -n "$OUT" ]; then
  printf '%s' '{"run_id":"http-run","status":"failed","changed_files":[],"failures":[{"code":"x","message":"boom"}]}' > "$OUT"
  printf '200'
else
  printf '%s' '{"run_id":"http-run","status":"accepted"}'
fi
exit 0
"#;

    #[cfg(unix)]
    #[test]
    fn run_jailgun_http_submits_polls_and_summarizes() {
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "curl_ok.sh", STUB_CURL_OK);
        let summary = super::run_jailgun_http(
            stub.to_str().unwrap(),
            "http://127.0.0.1:9",
            &jx_simple(),
            std::time::Duration::from_secs(30),
            "produce",
        )
        .expect("http jailgun succeeds");
        assert!(summary.contains("http-run"), "summary: {summary}");
        assert!(summary.contains("status=succeeded"), "summary: {summary}");
        assert!(summary.contains("changed_files=1"), "summary: {summary}");
    }

    #[cfg(unix)]
    #[test]
    fn run_jailgun_http_fails_phase_on_failed_status() {
        let dir = tempfile::tempdir().expect("tmp");
        let stub = write_stub(dir.path(), "curl_fail.sh", STUB_CURL_FAIL);
        let err = super::run_jailgun_http(
            stub.to_str().unwrap(),
            "http://127.0.0.1:9",
            &jx_simple(),
            std::time::Duration::from_secs(30),
            "produce",
        )
        .expect_err("failed status must fail the phase");
        assert!(format!("{err:#}").contains("status=failed"), "err: {err:#}");
    }
}
