//! SQLite-backed persistence for SuperWorkflow runs.
//!
//! All schema lives in [`SCHEMA`] and is applied idempotently by
//! [`SupervisorStore::open`] / [`SupervisorStore::open_in_memory`]. The store
//! is intentionally synchronous; async hosts should wrap with
//! `spawn_blocking`.
//!
//! Implementation is split across sibling modules:
//! - [`schema`]: canonical DDL constant.
//! - [`write`]: mutating queries (run init, status updates, sign-offs,
//!   memory + evidence appends).
//!
//! Read-side queries and the connection lifecycle live in this file.

mod read;
mod schema;
mod write;

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::model::PhaseStatus;

pub use schema::SCHEMA;

pub use read::{PhaseStatusRow, RunStatusReport, TaskStatusRow};

/// SQLite-backed supervisor store.
pub struct SupervisorStore {
    conn: Connection,
    db_path: PathBuf,
}

impl SupervisorStore {
    /// Open (or create) a store at `path`. Schema is applied idempotently.
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        let conn = Connection::open(&db_path)?;
        let store = Self { conn, db_path };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an in-memory store. Useful for tests and ramdisk-style runs.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn,
            db_path: PathBuf::from(":memory:"),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Apply [`SCHEMA`] idempotently.
    pub fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(SCHEMA)
    }

    /// Borrow the underlying connection (escape hatch for hosts).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Path the store was opened at (`:memory:` for in-memory stores).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Look up a single phase status.
    pub fn phase_status(
        &self,
        run_id: &str,
        phase_id: &str,
    ) -> rusqlite::Result<Option<PhaseStatus>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM zyal_super_phases WHERE run_id = ?1 AND phase_id = ?2",
                params![run_id, phase_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(raw.and_then(|s| PhaseStatus::parse(&s)))
    }

    /// Return phase ids in [`PhaseStatus::Complete`], sorted ascending.
    pub fn completed_phase_ids(&self, run_id: &str) -> rusqlite::Result<Vec<String>> {
        self.phase_ids_with_status(run_id, PhaseStatus::Complete)
    }

    /// Return phase ids in [`PhaseStatus::Blocked`], sorted ascending.
    pub fn blocked_phase_ids(&self, run_id: &str) -> rusqlite::Result<Vec<String>> {
        self.phase_ids_with_status(run_id, PhaseStatus::Blocked)
    }

    fn phase_ids_with_status(
        &self,
        run_id: &str,
        status: PhaseStatus,
    ) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT phase_id FROM zyal_super_phases \
             WHERE run_id = ?1 AND status = ?2 \
             ORDER BY phase_id",
        )?;
        let rows = stmt.query_map(params![run_id, status.as_str()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
