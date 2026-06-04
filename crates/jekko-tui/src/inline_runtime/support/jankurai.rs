// ── Phase M slash action helpers ─────────────────────────────────────────────

/// Spawn a PTY-backed single-shot `/audit` (or `/audit-check`) run and return an
/// `Receiver<ChatEvent>` shaped exactly like a backend turn — the in-flight
/// machinery + tool-card pipeline already render the stream. Always runs the
/// read-only external `jankurai audit --mode advisory` command.
///
/// Returns `Err(message)` when `jankurai` is not on PATH so the
/// caller can emit an honest error notice instead of staring at a hung spinner.
fn spawn_jankurai_turn(
    cancel: CancellationToken,
) -> std::result::Result<tokio::sync::mpsc::Receiver<ChatEvent>, String> {
    let Some(jankurai) = which_in_path("jankurai") else {
        return Err("jankurai not in PATH — install jankurai v1.5.1".to_string());
    };
    let program = jankurai.display().to_string();
    let args = JANKURAI_AUDIT_ARGS
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>();
    let label = "jankurai audit".to_string();

    let id = format!("jankurai-{}", std::process::id());
    // Keep `/audit` (and `/jankurai`, which aliases here) a purely local, instant
    // scan: skip jankurai's network update-check entirely. Without this the audit
    // does a `git ls-remote` against GitHub on every run — latency at best, and a
    // hang if git falls back to an interactive credential prompt on the tool PTY.
    // (`GIT_TERMINAL_PROMPT=0` is also defaulted by pty_runner as a backstop.)
    let pty_cmd = crate::engine::pty_runner::PtyCommand::new(id.clone(), label, program)
        .with_args(args)
        .with_env(vec![("JANKURAI_NO_UPDATE_CHECK".to_string(), "1".to_string())])
        .with_cancel(cancel);
    let (tool_tx, mut tool_rx) = tokio::sync::mpsc::channel::<crate::action::ToolEvent>(256);
    let (chat_tx, chat_rx) = tokio::sync::mpsc::channel::<ChatEvent>(256);
    let runner = tokio::spawn(crate::engine::pty_runner::run(pty_cmd, tool_tx));

    // Bridge tool events → chat events so the existing inline pipeline handles
    // the active-tool chip + tool-card rendering automatically.
    tokio::spawn(async move {
        while let Some(evt) = tool_rx.recv().await {
            let terminal_event = matches!(
                &evt,
                crate::action::ToolEvent::Complete { .. } | crate::action::ToolEvent::Fail { .. }
            );
            if chat_tx.send(ChatEvent::Tool(evt)).await.is_err() {
                break;
            }
            if terminal_event {
                // WHY: tool Complete/Fail closes the visible chip but the chat
                // turn also needs TurnComplete so the runtime unwinds in-flight.
                let _ = chat_tx.send(ChatEvent::TurnComplete).await;
                break;
            }
        }
        let _ = runner.await;
    });

    Ok(chat_rx)
}

/// Compatibility route for `/jankurai`: Jekko no longer vendors or runs a
/// Jankurai worker loop, so this aliases to the external read-only audit.
fn spawn_jankurai_cycle_turn(
    cancel: CancellationToken,
) -> std::result::Result<tokio::sync::mpsc::Receiver<ChatEvent>, String> {
    spawn_jankurai_turn(cancel)
}

/// Canonical args for the read-only external audit step.
const JANKURAI_AUDIT_ARGS: &[&str] = &[
    "audit",
    ".",
    "--mode",
    "advisory",
    "--json",
    ".jankurai/repo-score.json",
    "--md",
    ".jankurai/repo-score.md",
];

/// Render the latest `/jankurai-status` summary from `<cwd>/.jankurai/repo-score.json`.
/// Returns `(NoticeKind, body)` so the caller can pick a Warn vs Info colour.
fn render_jankurai_status() -> (NoticeKind, String) {
    match render_jankurai_status_lines() {
        Ok(lines) => (NoticeKind::Info, lines.join("\n")),
        Err((kind, body)) => (kind, body),
    }
}

fn render_jankurai_status_lines() -> std::result::Result<Vec<String>, (NoticeKind, String)> {
    let cwd = current_dir_or_dot();
    let path = latest_repo_score_path(&cwd);
    let text = match std::fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => {
            return Err((
                NoticeKind::Warn,
                format!("no {} found — run /jankurai first", path.display()),
            ));
        }
    };
    let summary = match parse_jankurai_score_json(&text) {
        Ok(v) => v,
        Err(err) => {
            return Err((
                NoticeKind::Error,
                format!("repo-score.json parse failed: {err}"),
            ));
        }
    };
    let mut lines = Vec::new();
    lines.push(format!("score: {} / 100", summary.score));
    lines.push(format!("raw score: {}", summary.raw_score));
    lines.push(format!("caps: {}", summary.caps_count));
    if !summary.caps.is_empty() {
        lines.push(format!("  {}", summary.caps.join(", ")));
    }
    lines.push(format!("hard findings: {}", summary.hard_findings));
    lines.push(format!("soft findings: {}", summary.soft_findings));
    lines.push(format!("findings: {}", summary.findings_count));
    if !summary.blockers.is_empty() {
        lines.push(format!("blockers: {}", summary.blockers.join(", ")));
    }
    if let Some(claimed) = summary.claimed_conformance_level {
        lines.push(format!("claimed: {claimed}"));
    }
    if let Some(observed) = summary.observed_conformance_level {
        lines.push(format!("observed: {observed}"));
    }
    if summary.dirty_worktree {
        lines.push("dirty worktree: yes".to_string());
    }
    if let Some(decision) = summary.conformance_decision {
        lines.push(format!("decision: {decision}"));
    }
    Ok(lines)
}

fn latest_repo_score_path(cwd: &std::path::Path) -> std::path::PathBuf {
    let current = cwd.join(".jankurai").join("repo-score.json");
    if current.exists() {
        return current;
    }
    cwd.join("agent").join("repo-score.json")
}
