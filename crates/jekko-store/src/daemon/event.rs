//! `daemon_event` table — one row per daemon event emitted in a run.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::StoreResult;

/// Row in `daemon_event`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonEventRow {
    /// Event id.
    pub id: String,
    /// FK to `daemon_run.id`.
    pub run_id: String,
    /// Iteration the event occurred in.
    pub iteration: i64,
    /// Event type tag.
    pub event_type: String,
    /// Payload (JSON).
    pub payload_json: serde_json::Value,
    /// Creation timestamp (ms since epoch).
    pub time_created: i64,
    /// Last-update timestamp (ms since epoch).
    pub time_updated: i64,
}

/// Insert a daemon_event row.
pub fn insert_event(conn: &Connection, row: &DaemonEventRow) -> StoreResult<()> {
    let payload = serde_json::to_string(&row.payload_json)?;
    conn.execute(
        "INSERT INTO daemon_event (id, run_id, iteration, event_type, payload_json, time_created, time_updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            row.id,
            row.run_id,
            row.iteration,
            row.event_type,
            payload,
            row.time_created,
            row.time_updated,
        ],
    )?;
    Ok(())
}

/// Page of daemon_event rows plus the cursor to resume after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonEventPage {
    /// Rows in this page, oldest first.
    pub events: Vec<DaemonEventRow>,
    /// Opaque resume cursor (`time_created` of the last row), or `None` when
    /// the page is the tail of the stream.
    pub next_cursor: Option<i64>,
}

/// List a page of daemon_event rows for a run, ordered oldest-first.
///
/// `since_ms` is an exclusive lower bound on `time_created` (pass `0` to read
/// from the beginning). `limit` caps the number of rows returned. The returned
/// [`DaemonEventPage::next_cursor`] is `Some(time_created)` of the final row
/// only when a full page was returned (i.e. more rows may exist), so callers
/// can paginate by feeding it back as the next `since_ms`.
pub fn list_events_for_run_since(
    conn: &Connection,
    run_id: &str,
    since_ms: i64,
    limit: usize,
) -> StoreResult<DaemonEventPage> {
    let capped = limit.clamp(1, 1000);
    let mut stmt = conn.prepare(
        "SELECT id, run_id, iteration, event_type, payload_json, time_created, time_updated
         FROM daemon_event
         WHERE run_id = ?1 AND time_created > ?2
         ORDER BY time_created ASC, id ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![run_id, since_ms, capped as i64], event_from_row)?;
    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    let next_cursor = if events.len() == capped {
        events.last().map(|row| row.time_created)
    } else {
        None
    };
    Ok(DaemonEventPage {
        events,
        next_cursor,
    })
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DaemonEventRow> {
    let payload_text: String = row.get(4)?;
    let payload_json: serde_json::Value = serde_json::from_str(&payload_text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(DaemonEventRow {
        id: row.get(0)?,
        run_id: row.get(1)?,
        iteration: row.get(2)?,
        event_type: row.get(3)?,
        payload_json,
        time_created: row.get(5)?,
        time_updated: row.get(6)?,
    })
}

/// List daemon_event rows for a run.
pub fn list_events_for_run(conn: &Connection, run_id: &str) -> StoreResult<Vec<DaemonEventRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, iteration, event_type, payload_json, time_created, time_updated
         FROM daemon_event WHERE run_id = ?1 ORDER BY time_created ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![run_id], |row| {
        let payload_text: String = row.get(4)?;
        let payload_json: serde_json::Value =
            serde_json::from_str(&payload_text).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
        Ok(DaemonEventRow {
            id: row.get(0)?,
            run_id: row.get(1)?,
            iteration: row.get(2)?,
            event_type: row.get(3)?,
            payload_json,
            time_created: row.get(5)?,
            time_updated: row.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
