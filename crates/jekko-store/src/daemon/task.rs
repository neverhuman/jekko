//! `daemon_task` table — one row per daemon-managed task.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{StoreError, StoreResult};

/// Row in `daemon_task`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonTaskRow {
    /// Task id.
    pub id: String,
    /// FK to `daemon_run.id`.
    pub run_id: String,
    /// External tracking id (e.g. issue number).
    pub external_id: Option<String>,
    /// Human-friendly title.
    pub title: String,
    /// Free-form task body (JSON).
    pub body_json: serde_json::Value,
    /// Status tag.
    pub status: String,
    /// Lane tag (`normal`, `parallel`, …).
    pub lane: String,
    /// Phase tag.
    pub phase: String,
    /// Difficulty score 0..1.
    pub difficulty_score: f64,
    /// Risk score 0..1.
    pub risk_score: f64,
    /// Readiness score 0..1.
    pub readiness_score: f64,
    /// Implementation confidence score 0..1.
    pub implementation_confidence: f64,
    /// Verification confidence score 0..1.
    pub verification_confidence: f64,
    /// Attempt counter.
    pub attempt_count: i64,
    /// Counter of consecutive no-progress attempts.
    pub no_progress_count: i64,
    /// Current incubator round.
    pub incubator_round: i64,
    /// Incubator status tag.
    pub incubator_status: String,
    /// Id of the artifact accepted into HEAD.
    pub accepted_artifact_id: Option<String>,
    /// Last-assessment payload (JSON), if any.
    pub last_assessment_json: Option<serde_json::Value>,
    /// Promotion result payload (JSON), if any.
    pub promotion_result_json: Option<serde_json::Value>,
    /// Reason the task is blocked, if any.
    pub blocked_reason: Option<String>,
    /// Priority weight.
    pub priority: i64,
    /// Worker currently leasing the task.
    pub lease_worker_id: Option<String>,
    /// Lease expiry timestamp (ms since epoch).
    pub lease_expires_at: Option<i64>,
    /// Paths currently locked by this task (JSON), if any.
    pub locked_paths_json: Option<serde_json::Value>,
    /// Evidence/artifacts payload (JSON), if any.
    pub evidence_json: Option<serde_json::Value>,
    /// Creation timestamp (ms since epoch).
    pub time_created: i64,
    /// Last-update timestamp (ms since epoch).
    pub time_updated: i64,
}

/// SELECT column list for `daemon_task`, in the exact order [`map_task_row`] decodes.
const TASK_COLUMNS: &str = "id, run_id, external_id, title, body_json, status, lane, phase, \
     difficulty_score, risk_score, readiness_score, implementation_confidence, \
     verification_confidence, attempt_count, no_progress_count, incubator_round, \
     incubator_status, accepted_artifact_id, last_assessment_json, promotion_result_json, \
     blocked_reason, priority, lease_worker_id, lease_expires_at, locked_paths_json, \
     evidence_json, time_created, time_updated";

/// Decode a fully-selected `daemon_task` row. Column order MUST match [`TASK_COLUMNS`].
fn map_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DaemonTaskRow> {
    let body_text: String = row.get(4)?;
    let body_json: serde_json::Value = serde_json::from_str(&body_text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let json_opt =
        |idx: usize, text: Option<String>| -> rusqlite::Result<Option<serde_json::Value>> {
            text.as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        idx,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })
        };
    let last_assessment_text: Option<String> = row.get(18)?;
    let promotion_text: Option<String> = row.get(19)?;
    let locked_text: Option<String> = row.get(24)?;
    let evidence_text: Option<String> = row.get(25)?;
    Ok(DaemonTaskRow {
        id: row.get(0)?,
        run_id: row.get(1)?,
        external_id: row.get(2)?,
        title: row.get(3)?,
        body_json,
        status: row.get(5)?,
        lane: row.get(6)?,
        phase: row.get(7)?,
        difficulty_score: row.get(8)?,
        risk_score: row.get(9)?,
        readiness_score: row.get(10)?,
        implementation_confidence: row.get(11)?,
        verification_confidence: row.get(12)?,
        attempt_count: row.get(13)?,
        no_progress_count: row.get(14)?,
        incubator_round: row.get(15)?,
        incubator_status: row.get(16)?,
        accepted_artifact_id: row.get(17)?,
        last_assessment_json: json_opt(18, last_assessment_text)?,
        promotion_result_json: json_opt(19, promotion_text)?,
        blocked_reason: row.get(20)?,
        priority: row.get(21)?,
        lease_worker_id: row.get(22)?,
        lease_expires_at: row.get(23)?,
        locked_paths_json: json_opt(24, locked_text)?,
        evidence_json: json_opt(25, evidence_text)?,
        time_created: row.get(26)?,
        time_updated: row.get(27)?,
    })
}

/// Insert or replace a daemon_task row.
pub fn upsert_task(conn: &Connection, row: &DaemonTaskRow) -> StoreResult<()> {
    let body = serde_json::to_string(&row.body_json)?;
    let last_assessment = row
        .last_assessment_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let promotion = row
        .promotion_result_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let locked = row
        .locked_paths_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let evidence = row
        .evidence_json
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    conn.execute(
        "INSERT INTO daemon_task (
            id, run_id, external_id, title, body_json, status, lane, phase,
            difficulty_score, risk_score, readiness_score,
            implementation_confidence, verification_confidence,
            attempt_count, no_progress_count, incubator_round, incubator_status,
            accepted_artifact_id, last_assessment_json, promotion_result_json,
            blocked_reason, priority, lease_worker_id, lease_expires_at,
            locked_paths_json, evidence_json, time_created, time_updated
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
            ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
        )
        ON CONFLICT(id) DO UPDATE SET
            external_id = excluded.external_id,
            title = excluded.title,
            body_json = excluded.body_json,
            status = excluded.status,
            lane = excluded.lane,
            phase = excluded.phase,
            difficulty_score = excluded.difficulty_score,
            risk_score = excluded.risk_score,
            readiness_score = excluded.readiness_score,
            implementation_confidence = excluded.implementation_confidence,
            verification_confidence = excluded.verification_confidence,
            attempt_count = excluded.attempt_count,
            no_progress_count = excluded.no_progress_count,
            incubator_round = excluded.incubator_round,
            incubator_status = excluded.incubator_status,
            accepted_artifact_id = excluded.accepted_artifact_id,
            last_assessment_json = excluded.last_assessment_json,
            promotion_result_json = excluded.promotion_result_json,
            blocked_reason = excluded.blocked_reason,
            priority = excluded.priority,
            lease_worker_id = excluded.lease_worker_id,
            lease_expires_at = excluded.lease_expires_at,
            locked_paths_json = excluded.locked_paths_json,
            evidence_json = excluded.evidence_json,
            time_updated = excluded.time_updated",
        params![
            row.id,
            row.run_id,
            row.external_id,
            row.title,
            body,
            row.status,
            row.lane,
            row.phase,
            row.difficulty_score,
            row.risk_score,
            row.readiness_score,
            row.implementation_confidence,
            row.verification_confidence,
            row.attempt_count,
            row.no_progress_count,
            row.incubator_round,
            row.incubator_status,
            row.accepted_artifact_id,
            last_assessment,
            promotion,
            row.blocked_reason,
            row.priority,
            row.lease_worker_id,
            row.lease_expires_at,
            locked,
            evidence,
            row.time_created,
            row.time_updated,
        ],
    )?;
    Ok(())
}

/// Read a daemon_task row.
pub fn get_task(conn: &Connection, id: &str) -> StoreResult<Option<DaemonTaskRow>> {
    conn.query_row(
        "SELECT id, run_id, external_id, title, body_json, status, lane, phase,
                difficulty_score, risk_score, readiness_score,
                implementation_confidence, verification_confidence,
                attempt_count, no_progress_count, incubator_round, incubator_status,
                accepted_artifact_id, last_assessment_json, promotion_result_json,
                blocked_reason, priority, lease_worker_id, lease_expires_at,
                locked_paths_json, evidence_json, time_created, time_updated
         FROM daemon_task WHERE id = ?1",
        params![id],
        map_task_row,
    )
    .optional()
    .map_err(StoreError::from)
}

/// Delete a daemon_task row.
pub fn delete_task(conn: &Connection, id: &str) -> StoreResult<usize> {
    Ok(conn.execute("DELETE FROM daemon_task WHERE id = ?1", params![id])?)
}

// ── Bounded durable-queue primitives ────────────────────────────────────────
// These power the in-process bounded worker pool: a fixed set of workers each
// `claim_next_task` → execute → `complete_task`, with `extend_lease` heartbeats
// and expired-lease reclaim for crash recovery. No worker is ever spawned beyond
// the configured pool size, and no task is ever run by two workers at once.

/// A task is *claimable* when it is freshly `queued`/`ready` with no live lease,
/// or when its lease has expired (crash recovery: a worker died mid-task). Ordered
/// by priority (desc) then age (asc) so the queue is fair and starvation-free.
pub fn list_claimable_tasks(
    conn: &Connection,
    run_id: &str,
    now_ms: i64,
    limit: usize,
) -> StoreResult<Vec<DaemonTaskRow>> {
    let sql = format!(
        "SELECT {} FROM daemon_task \
         WHERE run_id = ?1 AND ( \
             (status IN ('queued','ready') AND lease_worker_id IS NULL) \
             OR (lease_expires_at IS NOT NULL AND lease_expires_at < ?2 \
                 AND status IN ('queued','ready','running')) \
         ) \
         ORDER BY priority DESC, time_created ASC LIMIT ?3",
        TASK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![run_id, now_ms, limit as i64], map_task_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Atomically claim the highest-priority claimable task for `worker_id`, marking it
/// `running` with a fresh lease (`now_ms + lease_ttl_ms`) and bumping `attempt_count`.
///
/// The guarded `UPDATE` is the concurrency primitive: even if many workers select the
/// same candidate, the `WHERE` clause (unleased OR expired) lets exactly one win, so a
/// task is never executed by two workers at once. Returns `None` when the queue is drained.
pub fn claim_next_task(
    conn: &Connection,
    run_id: &str,
    worker_id: &str,
    now_ms: i64,
    lease_ttl_ms: i64,
) -> StoreResult<Option<DaemonTaskRow>> {
    let expires_at = now_ms + lease_ttl_ms;
    for candidate in list_claimable_tasks(conn, run_id, now_ms, 8)? {
        let updated = conn.execute(
            "UPDATE daemon_task \
             SET lease_worker_id = ?1, lease_expires_at = ?2, status = 'running', \
                 attempt_count = attempt_count + 1, time_updated = ?3 \
             WHERE id = ?4 AND ( \
                 (status IN ('queued','ready') AND lease_worker_id IS NULL) \
                 OR (lease_expires_at IS NOT NULL AND lease_expires_at < ?5 \
                     AND status IN ('queued','ready','running')) \
             )",
            params![worker_id, expires_at, now_ms, candidate.id, now_ms],
        )?;
        if updated == 1 {
            return get_task(conn, &candidate.id);
        }
        // Lost the race for this candidate — another worker grabbed it. Try the next.
    }
    Ok(None)
}

/// Extend the lease of a task the worker still owns (a heartbeat for long jobs, so it
/// is not reclaimed as a crash). Returns `false` if the worker no longer owns the lease.
pub fn extend_lease(
    conn: &Connection,
    id: &str,
    worker_id: &str,
    now_ms: i64,
    lease_ttl_ms: i64,
) -> StoreResult<bool> {
    let n = conn.execute(
        "UPDATE daemon_task SET lease_expires_at = ?1, time_updated = ?2 \
         WHERE id = ?3 AND lease_worker_id = ?4",
        params![now_ms + lease_ttl_ms, now_ms, id, worker_id],
    )?;
    Ok(n == 1)
}

/// Mark a leased task terminal (`done`/`failed`/…), clearing its lease. Owner-guarded:
/// only the worker holding the lease may complete it. Optionally records a result payload.
pub fn complete_task(
    conn: &Connection,
    id: &str,
    worker_id: &str,
    terminal_status: &str,
    result_json: Option<&serde_json::Value>,
    now_ms: i64,
) -> StoreResult<bool> {
    let result = result_json.map(serde_json::to_string).transpose()?;
    let n = conn.execute(
        "UPDATE daemon_task \
         SET status = ?1, lease_worker_id = NULL, lease_expires_at = NULL, \
             promotion_result_json = COALESCE(?2, promotion_result_json), time_updated = ?3 \
         WHERE id = ?4 AND lease_worker_id = ?5",
        params![terminal_status, result, now_ms, id, worker_id],
    )?;
    Ok(n == 1)
}

/// Release a leased task back to the queue under `requeue_status` (e.g. `queued`) without
/// completing it — for transient failures or graceful shutdown mid-claim. Owner-guarded.
pub fn release_task(
    conn: &Connection,
    id: &str,
    worker_id: &str,
    requeue_status: &str,
    now_ms: i64,
) -> StoreResult<bool> {
    let n = conn.execute(
        "UPDATE daemon_task \
         SET status = ?1, lease_worker_id = NULL, lease_expires_at = NULL, time_updated = ?2 \
         WHERE id = ?3 AND lease_worker_id = ?4",
        params![requeue_status, now_ms, id, worker_id],
    )?;
    Ok(n == 1)
}

/// Count tasks in `status` for a run (for backpressure + observability).
pub fn count_tasks_by_status(conn: &Connection, run_id: &str, status: &str) -> StoreResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM daemon_task WHERE run_id = ?1 AND status = ?2",
        params![run_id, status],
        |row| row.get(0),
    )?)
}

/// Build a fresh `queued` task row with default scores/counters. Producers fill
/// `body_json` with the job spec (e.g. `{ "prompt": "…" }`) and enqueue via
/// [`upsert_task`]. The bounded worker pool then claims and runs it.
pub fn queued_task(
    run_id: impl Into<String>,
    id: impl Into<String>,
    title: impl Into<String>,
    body_json: serde_json::Value,
    priority: i64,
    now_ms: i64,
) -> DaemonTaskRow {
    DaemonTaskRow {
        id: id.into(),
        run_id: run_id.into(),
        external_id: None,
        title: title.into(),
        body_json,
        status: "queued".to_string(),
        lane: "normal".to_string(),
        phase: "queued".to_string(),
        difficulty_score: 0.0,
        risk_score: 0.0,
        readiness_score: 0.0,
        implementation_confidence: 0.0,
        verification_confidence: 0.0,
        attempt_count: 0,
        no_progress_count: 0,
        incubator_round: 0,
        incubator_status: "none".to_string(),
        accepted_artifact_id: None,
        last_assessment_json: None,
        promotion_result_json: None,
        blocked_reason: None,
        priority,
        lease_worker_id: None,
        lease_expires_at: None,
        locked_paths_json: None,
        evidence_json: None,
        time_created: now_ms,
        time_updated: now_ms,
    }
}

/// Ensure a minimal `daemon_run` row exists so standalone queue tasks satisfy their
/// `run_id` foreign key. Uses placeholder session ids — callers using this for a
/// lightweight standalone queue should relax FK enforcement on the connection
/// (`PRAGMA foreign_keys = OFF`) since no real `session` rows back the placeholders.
pub fn ensure_placeholder_run(conn: &Connection, run_id: &str, now_ms: i64) -> StoreResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO daemon_run \
         (id, root_session_id, active_session_id, status, phase, spec_json, spec_hash, \
          iteration, epoch, time_created, time_updated) \
         VALUES (?1, 'queue', 'queue', 'running', 'queue', '{}', '', 0, 0, ?2, ?2)",
        params![run_id, now_ms],
    )?;
    Ok(())
}

#[cfg(test)]
mod queue_tests {
    use super::*;
    use crate::db::Db;

    /// In-memory DB with FK enforcement off, so queue tests can insert tasks
    /// directly without the full session → daemon_run parent chain.
    fn test_db() -> Db {
        let db = Db::open_in_memory().expect("open in-memory db");
        db.connection()
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("disable fk for isolated queue test");
        db
    }

    fn queued(id: &str, priority: i64) -> DaemonTaskRow {
        DaemonTaskRow {
            id: id.to_string(),
            run_id: "run-1".to_string(),
            external_id: None,
            title: format!("task {id}"),
            body_json: serde_json::json!({ "prompt": "noop" }),
            status: "queued".to_string(),
            lane: "normal".to_string(),
            phase: "queued".to_string(),
            difficulty_score: 0.0,
            risk_score: 0.0,
            readiness_score: 0.0,
            implementation_confidence: 0.0,
            verification_confidence: 0.0,
            attempt_count: 0,
            no_progress_count: 0,
            incubator_round: 0,
            incubator_status: "none".to_string(),
            accepted_artifact_id: None,
            last_assessment_json: None,
            promotion_result_json: None,
            blocked_reason: None,
            priority,
            lease_worker_id: None,
            lease_expires_at: None,
            locked_paths_json: None,
            evidence_json: None,
            time_created: 1_000,
            time_updated: 1_000,
        }
    }

    #[test]
    fn claim_picks_highest_priority_and_sets_lease() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        upsert_task(c, &queued("b", 5)).unwrap();
        upsert_task(c, &queued("c", 3)).unwrap();
        let now = 10_000;
        let claimed = claim_next_task(c, "run-1", "w1", now, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id, "b");
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.lease_worker_id.as_deref(), Some("w1"));
        assert_eq!(claimed.lease_expires_at, Some(now + 1_000));
        assert_eq!(claimed.attempt_count, 1);
    }

    #[test]
    fn two_claims_do_not_grab_same_task() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        upsert_task(c, &queued("b", 2)).unwrap();
        let now = 10_000;
        let first = claim_next_task(c, "run-1", "w1", now, 1_000)
            .unwrap()
            .unwrap();
        let second = claim_next_task(c, "run-1", "w2", now, 1_000)
            .unwrap()
            .unwrap();
        assert_ne!(first.id, second.id);
        // Pool drained: a third worker gets nothing.
        assert!(claim_next_task(c, "run-1", "w3", now, 1_000)
            .unwrap()
            .is_none());
    }

    #[test]
    fn active_lease_not_reclaimed_but_expired_is() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        let t0 = 10_000;
        let claimed = claim_next_task(c, "run-1", "w1", t0, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id, "a");
        // Still leased → nothing claimable before expiry.
        assert!(claim_next_task(c, "run-1", "w2", t0 + 500, 1_000)
            .unwrap()
            .is_none());
        // After expiry → another worker reclaims it (crash recovery).
        let reclaimed = claim_next_task(c, "run-1", "w2", t0 + 2_000, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(reclaimed.id, "a");
        assert_eq!(reclaimed.lease_worker_id.as_deref(), Some("w2"));
        assert_eq!(reclaimed.attempt_count, 2);
    }

    #[test]
    fn complete_is_owner_guarded_and_clears_lease() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        let now = 10_000;
        claim_next_task(c, "run-1", "w1", now, 1_000)
            .unwrap()
            .unwrap();
        // Wrong worker cannot complete.
        assert!(!complete_task(c, "a", "intruder", "done", None, now + 1).unwrap());
        // Owner completes with a result payload.
        assert!(complete_task(
            c,
            "a",
            "w1",
            "done",
            Some(&serde_json::json!({ "ok": true })),
            now + 2
        )
        .unwrap());
        let t = get_task(c, "a").unwrap().unwrap();
        assert_eq!(t.status, "done");
        assert!(t.lease_worker_id.is_none());
        assert!(t.lease_expires_at.is_none());
        assert_eq!(t.promotion_result_json, Some(serde_json::json!({ "ok": true })));
        // A completed task is no longer claimable.
        assert!(claim_next_task(c, "run-1", "w2", now + 3, 1_000)
            .unwrap()
            .is_none());
    }

    #[test]
    fn release_requeues_for_retry() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        let now = 10_000;
        claim_next_task(c, "run-1", "w1", now, 1_000)
            .unwrap()
            .unwrap();
        assert!(release_task(c, "a", "w1", "queued", now + 1).unwrap());
        let again = claim_next_task(c, "run-1", "w2", now + 2, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(again.id, "a");
        assert_eq!(again.attempt_count, 2);
    }

    #[test]
    fn extend_lease_pushes_expiry_for_owner_only() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        let now = 10_000;
        claim_next_task(c, "run-1", "w1", now, 1_000)
            .unwrap()
            .unwrap();
        assert!(extend_lease(c, "a", "w1", now + 500, 1_000).unwrap());
        let t = get_task(c, "a").unwrap().unwrap();
        assert_eq!(t.lease_expires_at, Some(now + 500 + 1_000));
        // Non-owner cannot extend.
        assert!(!extend_lease(c, "a", "w2", now + 600, 1_000).unwrap());
    }

    #[test]
    fn count_by_status_tracks_lifecycle() {
        let db = test_db();
        let c = db.connection();
        upsert_task(c, &queued("a", 1)).unwrap();
        upsert_task(c, &queued("b", 1)).unwrap();
        assert_eq!(count_tasks_by_status(c, "run-1", "queued").unwrap(), 2);
        claim_next_task(c, "run-1", "w1", 10_000, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(count_tasks_by_status(c, "run-1", "running").unwrap(), 1);
        assert_eq!(count_tasks_by_status(c, "run-1", "queued").unwrap(), 1);
    }
}
