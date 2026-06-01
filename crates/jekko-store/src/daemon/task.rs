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

/// Lean projection of a [`DaemonTaskRow`] for list endpoints. Deliberately
/// omits the large JSON columns so callers can enumerate a run's tasks
/// without loading every assessment/evidence blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonTaskSummaryRow {
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

/// List lean task summaries for a run, optionally filtered by `status` and/or
/// `lane`. Ordered by priority (desc) then creation time (asc) — matching the
/// `daemon_task_run_status_idx` ordering used by the scheduler.
pub fn list_task_summaries_for_run(
    conn: &Connection,
    run_id: &str,
    status: Option<&str>,
    lane: Option<&str>,
) -> StoreResult<Vec<DaemonTaskSummaryRow>> {
    let mut sql = String::from(
        "SELECT id, run_id, title, status, lane, phase, priority, attempt_count,
                time_created, time_updated
         FROM daemon_task WHERE run_id = ?1",
    );
    // Hold owned bind values so their borrows outlive the prepared statement.
    let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&run_id];
    if let Some(status) = status.as_ref() {
        let idx = binds.len() + 1;
        sql.push_str(&format!(" AND status = ?{idx}"));
        binds.push(status);
    }
    if let Some(lane) = lane.as_ref() {
        let idx = binds.len() + 1;
        sql.push_str(&format!(" AND lane = ?{idx}"));
        binds.push(lane);
    }
    sql.push_str(" ORDER BY priority DESC, time_created ASC, id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(binds.as_slice(), |row| {
        Ok(DaemonTaskSummaryRow {
            id: row.get(0)?,
            run_id: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            lane: row.get(4)?,
            phase: row.get(5)?,
            priority: row.get(6)?,
            attempt_count: row.get(7)?,
            time_created: row.get(8)?,
            time_updated: row.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
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
        |row| {
            let body_text: String = row.get(4)?;
            let body_json: serde_json::Value = serde_json::from_str(&body_text).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
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
        },
    )
    .optional()
    .map_err(StoreError::from)
}

/// Delete a daemon_task row.
pub fn delete_task(conn: &Connection, id: &str) -> StoreResult<usize> {
    Ok(conn.execute("DELETE FROM daemon_task WHERE id = ?1", params![id])?)
}
