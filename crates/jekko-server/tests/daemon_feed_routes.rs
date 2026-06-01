//! Read-only daemon feed endpoints:
//! `GET /api/v1/daemon/:run_id/events` and `GET /api/v1/daemon/:run_id/tasks`.
//!
//! These read from the existing durable `daemon_event` / `daemon_task` tables
//! via the default DB resolved from `JEKKO_DB`. This test owns its own binary
//! so the process-global `JEKKO_DB` it sets cannot race other server tests.

mod common;

use axum::http::StatusCode;
use common::{body_json, get, make_router};
use jekko_store::daemon::{self, DaemonEventRow, DaemonRunRow, DaemonTaskRow};
use jekko_store::project::{self, ProjectRow};
use jekko_store::session::{self, SessionRow};
use jekko_store::Db;
use serde_json::json;
use tower::ServiceExt;

fn seed_db(path: &std::path::Path) {
    let db = Db::open(path).expect("open db");
    let conn = db.connection();
    project::upsert(
        conn,
        &ProjectRow {
            id: "project-1".into(),
            worktree: "/tmp/project-1".into(),
            vcs: Some("git".into()),
            name: Some("project".into()),
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: 1,
            time_updated: 1,
            time_initialized: Some(1),
            sandboxes: Vec::new(),
            commands: None,
        },
    )
    .unwrap();
    session::upsert(
        conn,
        &SessionRow {
            id: "session-1".into(),
            project_id: "project-1".into(),
            workspace_id: None,
            parent_id: None,
            slug: "session-1".into(),
            directory: "/tmp/project-1".into(),
            path: None,
            title: "seed".into(),
            version: "v1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            agent: None,
            model: None,
            time_created: 1,
            time_updated: 1,
            time_compacting: None,
            time_archived: None,
        },
    )
    .unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    daemon::upsert_run(
        conn,
        &DaemonRunRow {
            id: "run-1".into(),
            root_session_id: "session-1".into(),
            active_session_id: "session-1".into(),
            status: "running".into(),
            phase: "drafting".into(),
            spec_json: json!({"kind": "port"}),
            spec_hash: "hash".into(),
            iteration: 1,
            epoch: 0,
            last_error: None,
            last_exit_result_json: None,
            stopped_at: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    for i in 0..3 {
        daemon::insert_event(
            conn,
            &DaemonEventRow {
                id: format!("evt-{i}"),
                run_id: "run-1".into(),
                iteration: 1,
                event_type: "daemon.task.created".into(),
                payload_json: json!({
                    "run_id": "run-1",
                    "ids": ["task-1"],
                    "timestamp_ms": 100 + i,
                    "idempotency_key": format!("idk_{i}"),
                }),
                time_created: 100 + i as i64,
                time_updated: 100 + i as i64,
            },
        )
        .unwrap();
    }
    daemon::upsert_task(
        conn,
        &DaemonTaskRow {
            id: "task-1".into(),
            run_id: "run-1".into(),
            external_id: None,
            title: "first task".into(),
            body_json: json!({"summary": "seed"}),
            status: "ready".into(),
            lane: "normal".into(),
            phase: "queued".into(),
            difficulty_score: 0.0,
            risk_score: 0.0,
            readiness_score: 0.0,
            implementation_confidence: 0.0,
            verification_confidence: 0.0,
            attempt_count: 0,
            no_progress_count: 0,
            incubator_round: 0,
            incubator_status: "none".into(),
            accepted_artifact_id: None,
            last_assessment_json: None,
            promotion_result_json: None,
            blocked_reason: None,
            priority: 10,
            lease_worker_id: None,
            lease_expires_at: None,
            locked_paths_json: None,
            evidence_json: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn events_and_tasks_endpoints_read_durable_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("jekko.db");
    seed_db(&db_path);
    // SAFETY: this test owns its own binary, so no other test runs in-process.
    std::env::set_var("JEKKO_DB", &db_path);

    let app = make_router(common::fresh_state());

    // Events: page from the beginning with a small limit -> resume cursor.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/daemon/run-1/events?since=0&limit=2"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let events = body["events"].as_array().expect("events array");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"], "evt-0");
    assert_eq!(events[0]["kind"], "daemon.task.created");
    assert_eq!(events[0]["timestamp_ms"], 100);
    assert_eq!(events[0]["idempotency_key"], "idk_0");
    assert_eq!(body["next_cursor"], 101);

    // Resume after the cursor -> remaining tail, no cursor.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/daemon/run-1/events?since=101&limit=10"))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["events"].as_array().unwrap().len(), 1);
    assert!(body["next_cursor"].is_null());

    // Tasks: lean projection, filterable.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/daemon/run-1/tasks"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["run_id"], "run-1");
    let tasks = body["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], "task-1");
    assert_eq!(tasks[0]["status"], "ready");
    assert_eq!(tasks[0]["lane"], "normal");
    // Lean shape: no embedded run / body blobs.
    assert!(tasks[0].get("body_json").is_none());

    let resp = app
        .oneshot(get("/api/v1/daemon/run-1/tasks?status=done"))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["tasks"].as_array().unwrap().len(), 0);

    std::env::remove_var("JEKKO_DB");
}
