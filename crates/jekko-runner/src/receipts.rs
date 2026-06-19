//! SQLite receipts store. Four tables:
//!   - `runs`: one row per `jekko-runner` invocation.
//!   - `commits`: every commit landed on the integration branch.
//!   - `findings`: classification snapshots per tick.
//!   - `events`: indexed mirror of the NDJSON event stream for fast queries.
//!
//! Mirrors the bundled `rusqlite` pattern from `crates/agent-search/src/store.rs`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub const RECEIPT_DB_REL: &str = "agent/zyal/receipts.sqlite";

pub struct ReceiptsStore {
    conn: Connection,
    path: PathBuf,
}

impl ReceiptsStore {
    pub fn open(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(RECEIPT_DB_REL);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        let store = Self { conn, path };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS runs (
                run_id TEXT PRIMARY KEY,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                pool_size INTEGER NOT NULL,
                integration_branch TEXT NOT NULL,
                dry_run INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'running'
            );
            CREATE INDEX IF NOT EXISTS runs_started_idx ON runs(started_at);

            CREATE TABLE IF NOT EXISTS commits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                worker_id TEXT NOT NULL,
                finding_id TEXT NOT NULL,
                rule_id TEXT NOT NULL,
                branch TEXT NOT NULL,
                sha TEXT NOT NULL,
                landed_at INTEGER NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS commits_run_idx ON commits(run_id);
            CREATE INDEX IF NOT EXISTS commits_sha_idx ON commits(sha);

            CREATE TABLE IF NOT EXISTS findings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                rule_id TEXT NOT NULL,
                severity TEXT NOT NULL,
                paths_json TEXT NOT NULL,
                cap TEXT,
                captured_at INTEGER NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS findings_run_idx ON findings(run_id);
            CREATE INDEX IF NOT EXISTS findings_fp_idx ON findings(fingerprint);

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                ts INTEGER NOT NULL,
                kind TEXT NOT NULL,
                data_json TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS events_run_idx ON events(run_id);
            CREATE INDEX IF NOT EXISTS events_kind_idx ON events(kind);
            "#,
        )?;
        Ok(())
    }

    pub fn record_run_started(
        &self,
        run_id: &str,
        pool_size: usize,
        integration_branch: &str,
        dry_run: bool,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO runs (run_id, started_at, pool_size, integration_branch, dry_run, status)
            VALUES (?1, ?2, ?3, ?4, ?5, 'running')
            "#,
            params![run_id, now_secs() as i64, pool_size as i64, integration_branch, dry_run as i64],
        )?;
        Ok(())
    }

    pub fn record_run_finished(&self, run_id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            r#"UPDATE runs SET finished_at = ?2, status = ?3 WHERE run_id = ?1"#,
            params![run_id, now_secs() as i64, status],
        )?;
        Ok(())
    }

    pub fn record_commit(
        &self,
        run_id: &str,
        worker_id: &str,
        finding_id: &str,
        rule_id: &str,
        branch: &str,
        sha: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO commits (run_id, worker_id, finding_id, rule_id, branch, sha, landed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                run_id,
                worker_id,
                finding_id,
                rule_id,
                branch,
                sha,
                now_secs() as i64
            ],
        )?;
        Ok(())
    }

    pub fn record_finding(
        &self,
        run_id: &str,
        fingerprint: &str,
        rule_id: &str,
        severity: &str,
        paths: &[String],
        cap: Option<&str>,
    ) -> Result<()> {
        let paths_json = serde_json::to_string(paths).context("serialize paths")?;
        self.conn.execute(
            r#"
            INSERT INTO findings (run_id, fingerprint, rule_id, severity, paths_json, cap, captured_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![run_id, fingerprint, rule_id, severity, paths_json, cap, now_secs() as i64],
        )?;
        Ok(())
    }

    pub fn record_event(&self, run_id: &str, ts: u64, kind: &str, data_json: &str) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO events (run_id, ts, kind, data_json) VALUES (?1, ?2, ?3, ?4)"#,
            params![run_id, ts as i64, kind, data_json],
        )?;
        Ok(())
    }

    pub fn count_commits_for_run(&self, run_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM commits WHERE run_id = ?1",
            params![run_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_all_tables() {
        let dir = tempdir().unwrap();
        let store = ReceiptsStore::open(dir.path()).unwrap();
        for table in TABLE_NAMES_FOR_INIT_TEST {
            let count: i64 = count_rows(&store.conn, table);
            assert_eq!(count, 0);
        }
    }

    fn count_rows(conn: &rusqlite::Connection, table: &'static str) -> i64 {
        // The `table` argument is a `&'static str` taken from the compile-time
        // closed set TABLE_NAMES_FOR_INIT_TEST asserted below; it cannot be
        // user-controlled. rusqlite identifiers cannot be parameterized, so we
        // interpolate, but only from this closed set.
        // jankurai:allow HLT-023-INPUT-BOUNDARY-GAP reason=identifier-from-compile-time-closed-set expires=2027-01-01
        let sql = format!("SELECT COUNT(*) FROM {}", table);
        conn.query_row(&sql, [], |r| r.get(0)).unwrap()
    }

    /// Closed set of table names this test smoke-counts. SQLite identifiers
    /// cannot be parameterized at the bind layer, so the counter helper
    /// interpolates the table name — but only from this static set, which the
    /// regression test below asserts contains no separators or whitespace.
    const TABLE_NAMES_FOR_INIT_TEST: &[&str] = &["runs", "commits", "findings", "events"];

    #[test]
    fn init_test_table_set_is_a_compile_time_closed_set() {
        // Mirrors the negative-input contract: the test above can never see a
        // value that wasn't compiled into the binary.
        assert!(TABLE_NAMES_FOR_INIT_TEST.iter().all(|t| !t.contains(';')));
        assert!(TABLE_NAMES_FOR_INIT_TEST.iter().all(|t| !t.contains(' ')));
    }

    #[test]
    fn records_a_run_and_commits() {
        let dir = tempdir().unwrap();
        let store = ReceiptsStore::open(dir.path()).unwrap();
        store
            .record_run_started("run-1", 4, "zyal/run-1/integration", false)
            .unwrap();
        store
            .record_commit(
                "run-1",
                "w-01",
                "fp1",
                "HLT-001",
                "zyal/run-1/w-01/x",
                "deadbeef",
            )
            .unwrap();
        store
            .record_commit(
                "run-1",
                "w-02",
                "fp2",
                "HLT-002",
                "zyal/run-1/w-02/y",
                "cafef00d",
            )
            .unwrap();
        assert_eq!(store.count_commits_for_run("run-1").unwrap(), 2);
        store.record_run_finished("run-1", "ok").unwrap();
    }

    #[test]
    fn records_findings_and_events() {
        let dir = tempdir().unwrap();
        let store = ReceiptsStore::open(dir.path()).unwrap();
        store
            .record_run_started("run-2", 2, "zyal/run-2/integration", true)
            .unwrap();
        store
            .record_finding(
                "run-2",
                "fp-x",
                "HLT-007",
                "high",
                &["src/a.rs".to_string()],
                None,
            )
            .unwrap();
        store
            .record_finding(
                "run-2",
                "cap-x",
                "cap:no-sec",
                "critical",
                &["agent/proof-lanes.toml".to_string()],
                Some("no-sec"),
            )
            .unwrap();
        store.record_event("run-2", 1, "run_started", "{}").unwrap();
        let findings_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM findings WHERE run_id = 'run-2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(findings_count, 2);
        let events_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE run_id = 'run-2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(events_count, 1);
    }

    #[test]
    fn replacing_a_run_keeps_started_at_overwritten() {
        let dir = tempdir().unwrap();
        let store = ReceiptsStore::open(dir.path()).unwrap();
        store
            .record_run_started("dup", 1, "zyal/dup/integration", false)
            .unwrap();
        store
            .record_run_started("dup", 8, "zyal/dup/integration", true)
            .unwrap();
        let pool: i64 = store
            .conn
            .query_row("SELECT pool_size FROM runs WHERE run_id = 'dup'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(pool, 8);
    }
}
