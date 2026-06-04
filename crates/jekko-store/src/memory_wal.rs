//! Durable [`jekko_memory::WalSink`] over the `cogcore_wal` SQLite table.
//!
//! Jekko memory owns the in-process cogcore service; the store owns durable
//! SQLite persistence. This module is intentionally limited to WAL load/append
//! and does not know about runners, sandboxes, or Jeryu worktrees.

use jekko_memory::{MemoryError, WalRecord, WalSink};
use rusqlite::params;

use crate::db::Db;

/// A [`WalSink`] backed by the `cogcore_wal` table of a [`Db`].
///
/// Rows are scoped by `scope` (for example `"global"` today, or
/// `"project:<id>"` for future federation).
pub struct SqliteWalSink {
    db: Db,
    scope: String,
}

impl SqliteWalSink {
    /// Wrap an existing [`Db`] with the given WAL scope.
    pub fn new(db: Db, scope: impl Into<String>) -> Self {
        Self {
            db,
            scope: scope.into(),
        }
    }

    /// Open a fresh in-memory database with migrations applied.
    pub fn open_in_memory(scope: impl Into<String>) -> Result<Self, MemoryError> {
        let db = Db::open_in_memory().map_err(|e| MemoryError::Sink(e.to_string()))?;
        Ok(Self::new(db, scope))
    }

    /// Borrow the underlying database.
    pub fn db(&self) -> &Db {
        &self.db
    }
}

impl WalSink for SqliteWalSink {
    fn persist(&mut self, records: &[WalRecord]) -> Result<(), MemoryError> {
        let scope = self.scope.clone();
        let tx = self
            .db
            .connection_mut()
            .transaction()
            .map_err(|e| MemoryError::Sink(e.to_string()))?;
        for record in records {
            tx.execute(
                "INSERT INTO cogcore_wal (scope, seq, op_json, prev_hash, hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &scope,
                    record.seq as i64,
                    &record.op_json,
                    &record.prev_hash,
                    &record.hash
                ],
            )
            .map_err(|e| MemoryError::Sink(e.to_string()))?;
        }
        tx.commit().map_err(|e| MemoryError::Sink(e.to_string()))?;
        Ok(())
    }

    fn load(&self) -> Result<Vec<WalRecord>, MemoryError> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT seq, op_json, prev_hash, hash FROM cogcore_wal \
                 WHERE scope = ?1 ORDER BY seq",
            )
            .map_err(|e| MemoryError::Sink(e.to_string()))?;
        let rows = stmt
            .query_map(params![&self.scope], |row| {
                Ok(WalRecord {
                    seq: row.get::<_, i64>(0)? as u64,
                    op_json: row.get(1)?,
                    prev_hash: row.get(2)?,
                    hash: row.get(3)?,
                })
            })
            .map_err(|e| MemoryError::Sink(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| MemoryError::Sink(e.to_string()))?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use jekko_memory::{MemoryConfig, MemoryService, TurnObservation};

    use super::*;

    fn turn(id: &str, session: &str, body: &str, time_ms: i64) -> TurnObservation {
        TurnObservation::new(id, session, "user", body, time_ms)
    }

    #[test]
    fn sqlite_walsink_survives_restart_with_byte_identical_state() {
        let sink = SqliteWalSink::open_in_memory("global").expect("open db");
        let mut first = MemoryService::new(sink, MemoryConfig::default());
        first
            .observe_turn(turn(
                "e1",
                "neutrino",
                "neutrinos have mass",
                1_700_000_000_000,
            ))
            .unwrap();
        first
            .observe_turn(turn(
                "e2",
                "neutrino",
                "ordering unknown",
                1_700_000_100_000,
            ))
            .unwrap();
        let _ = first.recall_block("neutrino", 1024);
        let state_hash = first.core().export_state_hash();

        let sink = first.into_sink();
        let mut restored = MemoryService::new(sink, MemoryConfig::default());
        restored.bootstrap().unwrap();
        assert_eq!(restored.core().export_state_hash(), state_hash);
    }

    #[test]
    fn empty_db_bootstrap_is_noop() {
        let sink = SqliteWalSink::open_in_memory("global").expect("open db");
        let mut memory = MemoryService::new(sink, MemoryConfig::default());
        memory.bootstrap().unwrap();
        assert!(memory.recall_block("anything", 256).is_none());
    }
}
