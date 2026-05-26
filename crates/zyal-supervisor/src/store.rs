//! SQLite-backed persistence for SuperWorkflow runs.
//!
//! All schema lives in [`SCHEMA`] and is applied idempotently by
//! [`SupervisorStore::open`] / [`SupervisorStore::open_in_memory`]. The store
//! is intentionally synchronous; async hosts should wrap with
//! `spawn_blocking`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{PhaseStatus, SuperWorkflow};

/// Canonical SQLite schema for the supervisor store.
///
/// Eight tables:
/// - `zyal_super_runs`        — one row per workflow run
/// - `zyal_super_phases`      — phase status + summary per run
/// - `zyal_super_tasks`       — materialized tasks per phase
/// - `zyal_super_memory`      — append-only memory rows
/// - `zyal_super_evidence`    — append-only evidence rows
/// - `zyal_super_repo_symbols`— per-run repo graph symbols
/// - `zyal_super_repo_edges`  — per-run repo graph edges
/// - `zyal_super_signoffs`    — sign-off verdicts per phase
pub const SCHEMA: &str = r#"
    PRAGMA foreign_keys = ON;

    CREATE TABLE IF NOT EXISTS zyal_super_runs (
        run_id        TEXT PRIMARY KEY,
        workflow_id   TEXT NOT NULL,
        name          TEXT NOT NULL,
        objective     TEXT NOT NULL,
        manifest_json TEXT NOT NULL,
        status        TEXT NOT NULL DEFAULT 'running',
        created_at    TEXT NOT NULL,
        updated_at    TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS zyal_super_phases (
        run_id         TEXT NOT NULL,
        phase_id       TEXT NOT NULL,
        name           TEXT NOT NULL,
        objective      TEXT NOT NULL,
        depends_on_json TEXT NOT NULL DEFAULT '[]',
        status         TEXT NOT NULL DEFAULT 'pending',
        summary        TEXT NOT NULL DEFAULT '',
        started_at     TEXT,
        completed_at   TEXT,
        updated_at     TEXT NOT NULL,
        PRIMARY KEY (run_id, phase_id),
        FOREIGN KEY (run_id) REFERENCES zyal_super_runs(run_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_tasks (
        run_id      TEXT NOT NULL,
        task_id     TEXT NOT NULL,
        phase_id    TEXT NOT NULL,
        title       TEXT NOT NULL,
        status      TEXT NOT NULL DEFAULT 'pending',
        owner       TEXT,
        lease_until INTEGER,
        scope_json  TEXT NOT NULL DEFAULT '{}',
        summary     TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL,
        updated_at  TEXT NOT NULL,
        PRIMARY KEY (run_id, task_id),
        FOREIGN KEY (run_id, phase_id) REFERENCES zyal_super_phases(run_id, phase_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_memory (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id     TEXT NOT NULL,
        phase_id   TEXT,
        task_id    TEXT,
        scope      TEXT NOT NULL,
        kind       TEXT NOT NULL,
        body       TEXT NOT NULL,
        tags_json  TEXT NOT NULL DEFAULT '[]',
        source_ref TEXT,
        created_at TEXT NOT NULL,
        FOREIGN KEY (run_id) REFERENCES zyal_super_runs(run_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_evidence (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id       TEXT NOT NULL,
        phase_id     TEXT,
        task_id      TEXT,
        kind         TEXT NOT NULL,
        uri          TEXT,
        content_hash TEXT,
        payload_json TEXT NOT NULL DEFAULT '{}',
        created_at   TEXT NOT NULL,
        FOREIGN KEY (run_id) REFERENCES zyal_super_runs(run_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_repo_symbols (
        run_id        TEXT NOT NULL,
        symbol_id     TEXT NOT NULL,
        kind          TEXT NOT NULL,
        path          TEXT NOT NULL,
        name          TEXT NOT NULL,
        span_json     TEXT NOT NULL DEFAULT '{}',
        metadata_json TEXT NOT NULL DEFAULT '{}',
        updated_at    TEXT NOT NULL,
        PRIMARY KEY (run_id, symbol_id),
        FOREIGN KEY (run_id) REFERENCES zyal_super_runs(run_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_repo_edges (
        run_id        TEXT NOT NULL,
        edge_id       TEXT NOT NULL,
        src_symbol_id TEXT NOT NULL,
        dst_symbol_id TEXT NOT NULL,
        kind          TEXT NOT NULL,
        weight        REAL NOT NULL DEFAULT 1.0,
        evidence_json TEXT NOT NULL DEFAULT '{}',
        updated_at    TEXT NOT NULL,
        PRIMARY KEY (run_id, edge_id),
        FOREIGN KEY (run_id) REFERENCES zyal_super_runs(run_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS zyal_super_signoffs (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id     TEXT NOT NULL,
        phase_id   TEXT NOT NULL,
        kind       TEXT NOT NULL,
        agent      TEXT NOT NULL,
        verdict    TEXT NOT NULL,
        notes      TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL,
        FOREIGN KEY (run_id, phase_id) REFERENCES zyal_super_phases(run_id, phase_id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS zyal_super_phase_status_idx
        ON zyal_super_phases(run_id, status);
    CREATE INDEX IF NOT EXISTS zyal_super_task_status_idx
        ON zyal_super_tasks(run_id, phase_id, status);
    CREATE INDEX IF NOT EXISTS zyal_super_memory_lookup_idx
        ON zyal_super_memory(run_id, scope, kind);
    CREATE INDEX IF NOT EXISTS zyal_super_evidence_lookup_idx
        ON zyal_super_evidence(run_id, kind, content_hash);
    CREATE INDEX IF NOT EXISTS zyal_super_repo_symbol_path_idx
        ON zyal_super_repo_symbols(run_id, path);
    CREATE INDEX IF NOT EXISTS zyal_super_repo_edge_kind_idx
        ON zyal_super_repo_edges(run_id, kind);
"#;

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

    /// Initialize a run from a manifest. Returns the assigned `run_id`.
    ///
    /// The run id is derived from the manifest id plus a millisecond
    /// timestamp so multiple runs of the same workflow coexist. All phases
    /// are seeded as [`PhaseStatus::Pending`]; callers can promote
    /// dependency-free phases to `Ready` via [`Self::record_phase_status`]
    /// or by reading [`crate::planner::ready_phases`].
    pub fn init_run(&self, manifest: &SuperWorkflow) -> rusqlite::Result<String> {
        let now_dt = Utc::now();
        let now = now_dt.to_rfc3339();
        let run_id = format!("{}-{}", manifest.id, now_dt.timestamp_millis());
        let manifest_json = serde_json::to_string(manifest)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;

        self.conn.execute(
            r#"
            INSERT INTO zyal_super_runs (
                run_id, workflow_id, name, objective, manifest_json, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?6)
            "#,
            params![
                run_id,
                manifest.id,
                manifest.name,
                manifest.objective,
                manifest_json,
                now,
            ],
        )?;

        for phase in &manifest.phases {
            let depends = serde_json::to_string(&phase.depends_on)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
            self.conn.execute(
                r#"
                INSERT INTO zyal_super_phases (
                    run_id, phase_id, name, objective, depends_on_json, status, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    run_id,
                    phase.id,
                    phase.name,
                    phase.objective,
                    depends,
                    PhaseStatus::Pending.as_str(),
                    now,
                ],
            )?;
        }

        Ok(run_id)
    }

    /// Update a phase status. Pass an empty `summary` to leave the existing
    /// summary unchanged.
    pub fn record_phase_status(
        &self,
        run_id: &str,
        phase_id: &str,
        status: PhaseStatus,
        summary: &str,
    ) -> rusqlite::Result<()> {
        let now = Utc::now().to_rfc3339();
        let completed_at: Option<&str> = if status == PhaseStatus::Complete {
            Some(now.as_str())
        } else {
            None
        };
        let started_at: Option<&str> = if status == PhaseStatus::Running {
            Some(now.as_str())
        } else {
            None
        };
        self.conn.execute(
            r#"
            UPDATE zyal_super_phases
            SET status = ?1,
                summary = CASE WHEN ?2 = '' THEN summary ELSE ?2 END,
                started_at = COALESCE(?3, started_at),
                completed_at = COALESCE(?4, completed_at),
                updated_at = ?5
            WHERE run_id = ?6 AND phase_id = ?7
            "#,
            params![
                status.as_str(),
                summary,
                started_at,
                completed_at,
                now,
                run_id,
                phase_id,
            ],
        )?;
        // Mirror the transition on the run row.
        self.conn.execute(
            "UPDATE zyal_super_runs SET updated_at = ?1 WHERE run_id = ?2",
            params![now, run_id],
        )?;
        Ok(())
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

    /// Record a sign-off verdict for a phase.
    pub fn record_signoff(
        &self,
        run_id: &str,
        phase_id: &str,
        kind: &str,
        agent: &str,
        verdict: &str,
        notes: &str,
    ) -> rusqlite::Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"
            INSERT INTO zyal_super_signoffs (
                run_id, phase_id, kind, agent, verdict, notes, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![run_id, phase_id, kind, agent, verdict, notes, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Append a memory row.
    #[allow(clippy::too_many_arguments)]
    pub fn append_memory(
        &self,
        run_id: &str,
        phase_id: Option<&str>,
        task_id: Option<&str>,
        scope: &str,
        kind: &str,
        body: &str,
        tags: &[String],
        source_ref: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let now = Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(tags)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.conn.execute(
            r#"
            INSERT INTO zyal_super_memory (
                run_id, phase_id, task_id, scope, kind, body, tags_json, source_ref, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![run_id, phase_id, task_id, scope, kind, body, tags_json, source_ref, now,],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Append an evidence row.
    #[allow(clippy::too_many_arguments)]
    pub fn append_evidence(
        &self,
        run_id: &str,
        phase_id: Option<&str>,
        task_id: Option<&str>,
        kind: &str,
        uri: Option<&str>,
        content_hash: Option<&str>,
        payload: &serde_json::Value,
    ) -> rusqlite::Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"
            INSERT INTO zyal_super_evidence (
                run_id, phase_id, task_id, kind, uri, content_hash, payload_json, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                run_id,
                phase_id,
                task_id,
                kind,
                uri,
                content_hash,
                payload.to_string(),
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ControllerPolicy, Gate, GateKind, MemoryPolicy, ParityPolicy, Phase, PhaseSignoffMode,
        RepoGraphPolicy, SandboxPolicy, SuperWorkflow, WriteScope,
    };

    fn sample_manifest() -> SuperWorkflow {
        let phases: Vec<Phase> = (0..9)
            .map(|i| Phase {
                id: format!("p{i:02}"),
                name: format!("Phase {i}"),
                objective: "objective".into(),
                depends_on: if i == 0 {
                    vec![]
                } else {
                    vec![format!("p{:02}", i - 1)]
                },
                write_scope: WriteScope::IsolatedWorktree,
                signoff: PhaseSignoffMode::Single,
                gates: vec![Gate {
                    name: "tests_green".into(),
                    kind: GateKind::TestsGreen,
                    required: true,
                }],
            })
            .collect();
        SuperWorkflow {
            id: "wf-store-test".into(),
            name: "Store Test".into(),
            objective: "Exercise the SQLite store".into(),
            phases,
            controller: ControllerPolicy::default(),
            memory: MemoryPolicy::default(),
            sandbox: SandboxPolicy::default(),
            repo_graph: RepoGraphPolicy::default(),
            parity: ParityPolicy::default(),
        }
    }

    #[test]
    fn init_schema_idempotent() {
        let store = SupervisorStore::open_in_memory().expect("open in-memory store");
        store.init_schema().expect("first call");
        store.init_schema().expect("second call must be a no-op");
        store.init_schema().expect("third call still a no-op");

        // Sanity-check that all 8 tables exist.
        let table_names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'zyal_super_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            table_names,
            vec![
                "zyal_super_evidence".to_string(),
                "zyal_super_memory".to_string(),
                "zyal_super_phases".to_string(),
                "zyal_super_repo_edges".to_string(),
                "zyal_super_repo_symbols".to_string(),
                "zyal_super_runs".to_string(),
                "zyal_super_signoffs".to_string(),
                "zyal_super_tasks".to_string(),
            ],
        );
    }

    #[test]
    fn init_run_persists_manifest() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest).expect("init_run");
        assert!(run_id.starts_with("wf-store-test-"));

        // Every phase is seeded pending.
        for phase in &manifest.phases {
            let status = store
                .phase_status(&run_id, &phase.id)
                .expect("query phase status")
                .expect("phase row must exist");
            assert_eq!(status, PhaseStatus::Pending);
        }

        // Round-trip the manifest_json column.
        let raw_json: String = store
            .connection()
            .query_row(
                "SELECT manifest_json FROM zyal_super_runs WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        let decoded: SuperWorkflow = serde_json::from_str(&raw_json).unwrap();
        assert_eq!(decoded.id, manifest.id);
        assert_eq!(decoded.phases.len(), manifest.phases.len());
    }

    #[test]
    fn record_phase_status_roundtrips() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest).unwrap();

        store
            .record_phase_status(&run_id, "p00", PhaseStatus::Running, "kicked off")
            .unwrap();
        assert_eq!(
            store.phase_status(&run_id, "p00").unwrap(),
            Some(PhaseStatus::Running),
        );

        store
            .record_phase_status(&run_id, "p00", PhaseStatus::Complete, "done")
            .unwrap();
        assert_eq!(
            store.phase_status(&run_id, "p00").unwrap(),
            Some(PhaseStatus::Complete),
        );

        // Empty summary preserves prior summary.
        store
            .record_phase_status(&run_id, "p00", PhaseStatus::Complete, "")
            .unwrap();
        let summary: String = store
            .connection()
            .query_row(
                "SELECT summary FROM zyal_super_phases WHERE run_id = ?1 AND phase_id = 'p00'",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(summary, "done");

        // record_signoff + append_memory + append_evidence smoke-check.
        let signoff_id = store
            .record_signoff(&run_id, "p00", "phase", "critic", "approved", "looks ok")
            .unwrap();
        assert!(signoff_id > 0);

        let memory_id = store
            .append_memory(
                &run_id,
                Some("p00"),
                None,
                "phase",
                "lesson",
                "always run parity",
                &["parity".to_string()],
                None,
            )
            .unwrap();
        assert!(memory_id > 0);

        let evidence_id = store
            .append_evidence(
                &run_id,
                Some("p00"),
                None,
                "proof",
                Some("target/proof.json"),
                Some("sha256:test"),
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        assert!(evidence_id > 0);
    }

    #[test]
    fn completed_phase_ids_returns_correct_set() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest).unwrap();

        // Nothing complete or blocked yet.
        assert!(store.completed_phase_ids(&run_id).unwrap().is_empty());
        assert!(store.blocked_phase_ids(&run_id).unwrap().is_empty());

        store
            .record_phase_status(&run_id, "p00", PhaseStatus::Complete, "done")
            .unwrap();
        store
            .record_phase_status(&run_id, "p01", PhaseStatus::Complete, "done")
            .unwrap();
        store
            .record_phase_status(&run_id, "p02", PhaseStatus::Blocked, "external dep")
            .unwrap();

        let completed = store.completed_phase_ids(&run_id).unwrap();
        assert_eq!(completed, vec!["p00".to_string(), "p01".to_string()]);

        let blocked = store.blocked_phase_ids(&run_id).unwrap();
        assert_eq!(blocked, vec!["p02".to_string()]);
    }
}
