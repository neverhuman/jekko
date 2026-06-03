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

mod schema;
mod write;

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::model::PhaseStatus;

pub use schema::SCHEMA;

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
        let run_id = store.init_run(&manifest, None).expect("init_run");
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
    fn init_run_honors_requested_id() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store
            .init_run(&manifest, Some("my-explicit-rid"))
            .expect("init_run with requested id");
        assert_eq!(run_id, "my-explicit-rid");

        // Phases still seeded under the requested id.
        for phase in &manifest.phases {
            let status = store
                .phase_status(&run_id, &phase.id)
                .expect("query phase status")
                .expect("phase row must exist");
            assert_eq!(status, PhaseStatus::Pending);
        }
    }

    #[test]
    fn record_phase_status_roundtrips() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest, None).unwrap();

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
        let run_id = store.init_run(&manifest, None).unwrap();

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
