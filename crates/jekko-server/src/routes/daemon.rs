//! `/api/v1/daemon` — daemon control + status.
//!
//! Ports the subset of `packages/jekko/src/server/routes/instance/httpapi/handlers/daemon.ts`
//! that does not yet require the runtime daemon trait. CRUD operates against
//! [`crate::state::DaemonRegistry`] which the runtime will eventually feed.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ServerError, ServerResult};
use crate::state::AppState;

/// Default page size for the daemon event feed.
const DEFAULT_EVENT_LIMIT: usize = 100;
/// Maximum page size for the daemon event feed.
const MAX_EVENT_LIMIT: usize = 1000;

/// Request body for `POST /api/v1/daemon/preview`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PreviewBody {
    /// Free-form text to preview.
    pub text: String,
}

/// Query parameters for `GET /api/v1/daemon/{run_id}/events`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct EventsQuery {
    /// Exclusive lower bound on `timestamp_ms` (ms since epoch). Defaults to 0.
    pub since: Option<i64>,
    /// Maximum number of events to return (1..=1000). Defaults to 100.
    pub limit: Option<usize>,
}

/// One event in the replayable daemon feed.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaemonEventView {
    /// Event id (durable row id).
    pub id: String,
    /// Event kind tag.
    pub kind: String,
    /// Event timestamp (ms since epoch).
    pub timestamp_ms: i64,
    /// Full event payload (JSON).
    pub properties: serde_json::Value,
    /// Idempotency key projected out of the payload, when present.
    pub idempotency_key: Option<String>,
}

/// Paginated response for `GET /api/v1/daemon/{run_id}/events`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaemonEventsResponse {
    /// Events in this page, oldest first.
    pub events: Vec<DaemonEventView>,
    /// Cursor to resume after, or `null` at the tail of the stream.
    pub next_cursor: Option<i64>,
}

/// Query parameters for `GET /api/v1/daemon/{run_id}/tasks`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct TasksQuery {
    /// Filter by task status tag.
    pub status: Option<String>,
    /// Filter by task lane tag.
    pub lane: Option<String>,
}

/// Lean task projection returned by `GET /api/v1/daemon/{run_id}/tasks`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaemonTaskView {
    /// Task id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Human-friendly title.
    pub title: String,
    /// Status tag.
    pub status: String,
    /// Lane tag.
    pub lane: String,
    /// Phase tag.
    pub phase: String,
    /// Priority weight.
    pub priority: i64,
    /// Attempt counter.
    pub attempt_count: i64,
    /// Creation timestamp (ms since epoch).
    pub time_created: i64,
    /// Last-update timestamp (ms since epoch).
    pub time_updated: i64,
}

/// Response for `GET /api/v1/daemon/{run_id}/tasks`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaemonTasksResponse {
    /// Run id the tasks belong to.
    pub run_id: String,
    /// Lean task list.
    pub tasks: Vec<DaemonTaskView>,
}

/// Build the daemon router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list))
        .route("/preview", post(preview))
        .route("/:run_id", get(get_run))
        .route("/:run_id/events", get(events))
        .route("/:run_id/tasks", get(tasks))
        .route("/:run_id/pause", post(pause))
        .route("/:run_id/resume", post(resume))
        .route("/:run_id/abort", post(abort))
}

/// `GET /api/v1/daemon` — list runs.
#[utoipa::path(
    get,
    path = "/api/v1/daemon",
    responses((status = 200, description = "Daemon runs"))
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<Vec<serde_json::Value>>> {
    let snap = state.daemons.read().await;
    let mut rows: Vec<serde_json::Value> = snap.runs.values().cloned().collect();
    if let Some(db) = open_default_db()? {
        for run in jekko_store::daemon::list_runs(db.connection(), 100)
            .map_err(|err| ServerError::internal(err.to_string()))?
        {
            rows.push(durable_run_value(&db, run)?);
        }
    }
    Ok(Json(rows))
}

/// `POST /api/v1/daemon/preview` — synthesise a preview without executing.
#[utoipa::path(
    post,
    path = "/api/v1/daemon/preview",
    request_body = PreviewBody,
    responses((status = 200, description = "Preview"))
)]
pub async fn preview(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<PreviewBody>,
) -> ServerResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "text": payload.text,
        "preview": {
            "summary": "preview deferred (daemon facade pending)",
        },
    })))
}

/// `GET /api/v1/daemon/:run_id`.
#[utoipa::path(
    get,
    path = "/api/v1/daemon/{run_id}",
    responses(
        (status = 200, description = "Daemon run"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    let snap = state.daemons.read().await;
    match snap.runs.get(&run_id).cloned() {
        Some(v) => Ok(Json(v)),
        None => {
            drop(snap);
            let Some(db) = open_default_db()? else {
                return Err(ServerError::not_found(format!("daemon: {run_id}")));
            };
            let Some(run) = jekko_store::daemon::get_run(db.connection(), &run_id)
                .map_err(|err| ServerError::internal(err.to_string()))?
            else {
                return Err(ServerError::not_found(format!("daemon: {run_id}")));
            };
            Ok(Json(durable_run_value(&db, run)?))
        }
    }
}

/// `GET /api/v1/daemon/{run_id}/events` — paginate the durable event feed.
///
/// Reads the existing `daemon_event` table oldest-first, returning a stable,
/// replayable page plus a `next_cursor` (a `timestamp_ms`) to resume after.
/// The `idempotency_key` is projected out of each event payload when present.
#[utoipa::path(
    get,
    path = "/api/v1/daemon/{run_id}/events",
    params(
        ("run_id" = String, Path, description = "Daemon run id"),
        EventsQuery,
    ),
    responses(
        (status = 200, description = "Replayable event page", body = DaemonEventsResponse),
        (status = 404, description = "Run not found"),
    )
)]
pub async fn events(
    State(_state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> ServerResult<Json<DaemonEventsResponse>> {
    let Some(db) = open_default_db()? else {
        return Err(ServerError::not_found(format!("daemon: {run_id}")));
    };
    let since = query.since.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(1, MAX_EVENT_LIMIT);
    let page = jekko_store::daemon::list_events_for_run_since(db.connection(), &run_id, since, limit)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let events = page
        .events
        .into_iter()
        .map(|row| {
            let idempotency_key = row
                .payload_json
                .get(jekko_runtime::daemon::IDEMPOTENCY_KEY_FIELD)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            DaemonEventView {
                id: row.id,
                kind: row.event_type,
                timestamp_ms: row.time_created,
                properties: row.payload_json,
                idempotency_key,
            }
        })
        .collect();
    Ok(Json(DaemonEventsResponse {
        events,
        next_cursor: page.next_cursor,
    }))
}

/// `GET /api/v1/daemon/{run_id}/tasks` — lean task list for a run.
///
/// Reads the existing `daemon_task` table and returns a slim projection
/// (no embedded run / assessment / evidence blobs), optionally filtered by
/// `status` and/or `lane`.
#[utoipa::path(
    get,
    path = "/api/v1/daemon/{run_id}/tasks",
    params(
        ("run_id" = String, Path, description = "Daemon run id"),
        TasksQuery,
    ),
    responses(
        (status = 200, description = "Lean task list", body = DaemonTasksResponse),
        (status = 404, description = "Run not found"),
    )
)]
pub async fn tasks(
    State(_state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Query(query): Query<TasksQuery>,
) -> ServerResult<Json<DaemonTasksResponse>> {
    let Some(db) = open_default_db()? else {
        return Err(ServerError::not_found(format!("daemon: {run_id}")));
    };
    let rows = jekko_store::daemon::list_task_summaries_for_run(
        db.connection(),
        &run_id,
        query.status.as_deref(),
        query.lane.as_deref(),
    )
    .map_err(|err| ServerError::internal(err.to_string()))?;
    let tasks = rows
        .into_iter()
        .map(|row| DaemonTaskView {
            id: row.id,
            run_id: row.run_id,
            title: row.title,
            status: row.status,
            lane: row.lane,
            phase: row.phase,
            priority: row.priority,
            attempt_count: row.attempt_count,
            time_created: row.time_created,
            time_updated: row.time_updated,
        })
        .collect();
    Ok(Json(DaemonTasksResponse { run_id, tasks }))
}

fn durable_run_value(
    db: &jekko_store::Db,
    run: jekko_store::daemon::DaemonRunRow,
) -> ServerResult<serde_json::Value> {
    let conn = db.connection();
    let targets = jekko_store::daemon::list_port_targets_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let mut target_values = Vec::new();
    let mut current_stage_count = 0_usize;
    let mut parity_seed_count = 0_usize;
    let mut last_jankurai_score = None;
    for target in targets {
        let phases = jekko_store::daemon::list_port_phases_for_target(conn, &target.id)
            .map_err(|err| ServerError::internal(err.to_string()))?;
        current_stage_count += phases.len();
        let mut phase_values = Vec::new();
        for phase in phases {
            let tasks = jekko_store::daemon::list_port_tasks_for_phase(conn, &phase.id)
                .map_err(|err| ServerError::internal(err.to_string()))?;
            phase_values.push(serde_json::json!({
                "phase": phase,
                "tasks": tasks,
            }));
        }
        let cases = jekko_store::daemon::list_parity_cases_for_target(conn, &target.id)
            .map_err(|err| ServerError::internal(err.to_string()))?;
        parity_seed_count += cases.len();
        let parity_runs = jekko_store::daemon::list_parity_runs_for_target(conn, &target.id)
            .map_err(|err| ServerError::internal(err.to_string()))?;
        last_jankurai_score = target.last_audit_score.or(last_jankurai_score);
        target_values.push(serde_json::json!({
            "target": target,
            "phases": phase_values,
            "parity_cases": cases,
            "parity_runs": parity_runs,
        }));
    }
    let graph_nodes = jekko_store::daemon::list_repo_graph_nodes_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let graph_edges = jekko_store::daemon::list_repo_graph_edges_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for node in &graph_nodes {
        *by_kind.entry(node.kind.clone()).or_insert(0) += 1;
    }
    let model_outcomes = jekko_store::daemon::list_model_outcomes_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let live_max_calls = run
        .spec_json
        .get("live_call_budget")
        .and_then(|budget| budget.get("max_calls"))
        .and_then(serde_json::Value::as_u64);
    let live_calls_used = model_outcomes.len() as u64;
    let last_model_kind = model_outcomes.last().map(|outcome| outcome.role.clone());
    let current_proof = run
        .spec_json
        .get("proofs")
        .and_then(serde_json::Value::as_object)
        .map(|proofs| {
            proofs
                .iter()
                .filter(|(_, value)| value.as_bool() == Some(true))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reasoning_artifacts = jekko_store::daemon::list_reasoning_artifacts_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let reasoning_lanes = jekko_store::daemon::list_reasoning_lanes_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let memory_capsules = jekko_store::daemon::list_memory_capsules_for_run(conn, &run.id)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let model_reliability = jekko_store::daemon::list_model_reliability(conn, None)
        .map_err(|err| ServerError::internal(err.to_string()))?;
    let parity_gap_count = target_values
        .iter()
        .flat_map(|target| {
            target
                .get("parity_runs")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|run| run.get("summary_json"))
        .filter_map(|summary| summary.get("gaps"))
        .filter_map(serde_json::Value::as_array)
        .map(Vec::len)
        .sum::<usize>();
    let benchmark_winner = reasoning_artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "reasoning_benchmark")
        .and_then(|artifact| artifact.payload_json.as_ref())
        .and_then(|payload| payload.get("winner"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let hero_judge_status = run
        .last_exit_result_json
        .as_ref()
        .filter(|value| value.get("generation").is_some() && value.get("hero_lane_count").is_some())
        .map(|value| {
            serde_json::json!({
                "generation": value.get("generation"),
                "hero_lane_count": value.get("hero_lane_count"),
                "judge_lane_count": value.get("judge_lane_count"),
                "frontier_winner": value.get("frontier_winner"),
                "knowledge_entry_count": value.get("knowledge_entry_count"),
                "search_receipt_count": value.get("search_receipt_count"),
                "last_promotion_decision": value.get("last_promotion_decision"),
                "quality_metrics_jsonl": value.get("quality_metrics_jsonl"),
                "quality_metrics_csv": value.get("quality_metrics_csv"),
                "quality_trend_json": value.get("quality_trend_json"),
                "lane_metrics_jsonl": value.get("lane_metrics_jsonl"),
                "lane_metrics_csv": value.get("lane_metrics_csv"),
                "hero_metrics_csv": value.get("hero_metrics_csv"),
                "judge_metrics_csv": value.get("judge_metrics_csv"),
                "reviewer_packet_json": value.get("reviewer_packet_json"),
                "model_budget": {
                    "used": value.get("model_calls_used"),
                    "max": value.get("model_call_budget"),
                },
                "last_model_kind": value.get("last_model_kind"),
            })
        });
    Ok(serde_json::json!({
        "source": "sqlite",
        "run": run,
        "port": {
            "targets": target_values,
            "repo_graph": {
                "nodes": graph_nodes.len(),
                "edges": graph_edges.len(),
                "by_kind": by_kind,
            },
            "model_outcomes": model_outcomes,
            "live_calls": {
                "used": live_calls_used,
                "remaining": live_max_calls.map(|max| max.saturating_sub(live_calls_used)),
            },
            "current_proof": current_proof,
            "last_model_kind": last_model_kind,
            "benchmark_winner": benchmark_winner,
            "current_stage_count": current_stage_count,
            "parity_seed_count": parity_seed_count,
            "last_jankurai_score": last_jankurai_score,
            "reasoning_artifacts": reasoning_artifacts,
            "reasoning_lanes": reasoning_lanes,
            "memory_capsule_count": memory_capsules.len(),
            "model_reliability_winners": model_reliability,
            "parity_gap_summary": {
                "gap_count": parity_gap_count,
            },
        }
        ,
        "hero_judge": hero_judge_status,
    }))
}

fn open_default_db() -> ServerResult<Option<jekko_store::Db>> {
    let path = db_path();
    if !path.exists() {
        return Ok(None);
    }
    jekko_store::Db::open(&path)
        .map(Some)
        .map_err(|err| ServerError::internal(err.to_string()))
}

fn db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("JEKKO_DB") {
        return path.into();
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".jekko").join("jekko.db"),
        None => PathBuf::from("jekko.db"),
    }
}

async fn publish_action(state: &AppState, run_id: &str, action: &str) -> ServerResult<Json<bool>> {
    let _ = state
        .bus
        .publish(
            "daemon.action",
            serde_json::json!({ "runID": run_id, "action": action }),
        )
        .await;
    Ok(Json(true))
}

/// `POST /api/v1/daemon/:run_id/pause`.
#[utoipa::path(
    post,
    path = "/api/v1/daemon/{run_id}/pause",
    responses((status = 200, description = "Paused"))
)]
pub async fn pause(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ServerResult<Json<bool>> {
    publish_action(&state, &run_id, "pause").await
}

/// `POST /api/v1/daemon/:run_id/resume`.
#[utoipa::path(
    post,
    path = "/api/v1/daemon/{run_id}/resume",
    responses((status = 200, description = "Resumed"))
)]
pub async fn resume(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ServerResult<Json<bool>> {
    publish_action(&state, &run_id, "resume").await
}

/// `POST /api/v1/daemon/:run_id/abort`.
#[utoipa::path(
    post,
    path = "/api/v1/daemon/{run_id}/abort",
    responses((status = 200, description = "Aborted"))
)]
pub async fn abort(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ServerResult<Json<bool>> {
    publish_action(&state, &run_id, "abort").await
}
