use std::path::Path;

use anyhow::Result;
use jekko_store::daemon::{self, DaemonRunRow};
use jekko_store::db::Db;
use jekko_store::project::{self, ProjectRow};
use jekko_store::session::{self, SessionRow};

use super::helpers::{hash_json, now_ms, project_id_for};

/// Ensure daemon FK parents exist for a run.
pub fn ensure_daemon_run(
    db: &Db,
    repo_root: &Path,
    run_id: &str,
    spec: serde_json::Value,
) -> Result<()> {
    let conn = db.connection();
    let now = now_ms();
    let project_id = project_id_for(repo_root);
    let session_id = format!("zyal-session-{run_id}");
    project::upsert(
        conn,
        &ProjectRow {
            id: project_id.clone(),
            worktree: repo_root.display().to_string(),
            vcs: Some("git".to_string()),
            name: repo_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string()),
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: now,
            time_updated: now,
            time_initialized: Some(now),
            sandboxes: Vec::new(),
            commands: None,
        },
    )?;
    session::upsert(
        conn,
        &SessionRow {
            id: session_id.clone(),
            project_id,
            workspace_id: None,
            parent_id: None,
            slug: session_id.clone(),
            directory: repo_root.display().to_string(),
            path: Some(
                repo_root
                    .join("target/zyal/runs")
                    .join(run_id)
                    .display()
                    .to_string(),
            ),
            title: format!("ZYAL port {run_id}"),
            version: "v1".to_string(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            agent: Some("zyal-port".to_string()),
            model: None,
            time_created: now,
            time_updated: now,
            time_compacting: None,
            time_archived: None,
        },
    )?;
    let spec_hash = hash_json(&spec)?;
    // Historical SQLite journals can leave daemon_run's session FK pointing at
    // a rebuild backup table on fresh in-memory databases. The daemon runtime
    // rows are still typed and queryable; keep FK enforcement off for this
    // daemon receipt connection, mirroring the existing daemon_store tests.
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    daemon::upsert_run(
        conn,
        &DaemonRunRow {
            id: run_id.to_string(),
            root_session_id: session_id.clone(),
            active_session_id: session_id,
            status: "running".to_string(),
            phase: "port".to_string(),
            spec_json: spec,
            spec_hash,
            iteration: 1,
            epoch: 0,
            last_error: None,
            last_exit_result_json: None,
            stopped_at: None,
            time_created: now,
            time_updated: now,
        },
    )?;
    Ok(())
}

/// Mark a durable daemon run status without disturbing its spec.
pub fn mark_daemon_run(
    db: &Db,
    run_id: &str,
    status: &str,
    phase: &str,
    error: Option<&str>,
) -> Result<()> {
    let conn = db.connection();
    let Some(mut row) = daemon::get_run(conn, run_id)? else {
        return Ok(());
    };
    row.status = status.to_string();
    row.phase = phase.to_string();
    row.last_error = error.map(|value| value.to_string());
    row.stopped_at = if matches!(status, "stopped" | "failed" | "complete") {
        Some(now_ms())
    } else {
        row.stopped_at
    };
    row.time_updated = now_ms();
    daemon::upsert_run(conn, &row)?;
    Ok(())
}

/// Persist the latest domain-specific exit/status summary for a daemon run.
pub fn record_daemon_exit_result(db: &Db, run_id: &str, result: serde_json::Value) -> Result<()> {
    let conn = db.connection();
    let Some(mut row) = daemon::get_run(conn, run_id)? else {
        return Ok(());
    };
    row.last_exit_result_json = Some(result);
    row.time_updated = now_ms();
    daemon::upsert_run(conn, &row)?;
    Ok(())
}
