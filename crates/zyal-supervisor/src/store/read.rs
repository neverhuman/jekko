use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::model::SuperWorkflow;

use super::SupervisorStore;

/// Persisted phase row surfaced by `port-run --status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseStatusRow {
    /// Phase identifier.
    pub phase_id: String,
    /// Display name.
    pub name: String,
    /// Phase objective.
    pub objective: String,
    /// Dependency list decoded from `depends_on_json`.
    pub depends_on: Vec<String>,
    /// Stored lifecycle status.
    pub status: String,
    /// Stored summary text.
    pub summary: String,
    /// Optional start timestamp.
    pub started_at: Option<String>,
    /// Optional completion timestamp.
    pub completed_at: Option<String>,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Persisted task row surfaced by `port-run --status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStatusRow {
    /// Task identifier.
    pub task_id: String,
    /// Owning phase id.
    pub phase_id: String,
    /// Task title.
    pub title: String,
    /// Stored task status.
    pub status: String,
    /// Optional owner.
    pub owner: Option<String>,
    /// Stored summary text.
    pub summary: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// JSON-serializable status report for a persisted run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStatusReport {
    /// Persisted run id.
    pub run_id: String,
    /// Phase rows sorted by `phase_id`.
    pub phases: Vec<PhaseStatusRow>,
    /// Task rows sorted by `phase_id`, `task_id`.
    pub tasks: Vec<TaskStatusRow>,
}

impl SupervisorStore {
    /// Return the persisted manifest for `run_id`.
    pub fn manifest_for_run(&self, run_id: &str) -> rusqlite::Result<Option<SuperWorkflow>> {
        let raw: Option<String> = self
            .connection()
            .query_row(
                "SELECT manifest_json FROM zyal_super_runs WHERE run_id = ?1",
                rusqlite::params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match raw {
            Some(raw_manifest) => {
                let manifest = serde_json::from_str(&raw_manifest).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok(Some(manifest))
            }
            None => Ok(None),
        }
    }

    /// Return phase and task rows for `run_id`, ordered for status output.
    pub fn status_report(&self, run_id: &str) -> rusqlite::Result<RunStatusReport> {
        let mut phase_stmt = self.connection().prepare(
            "SELECT phase_id, name, objective, depends_on_json, status, summary, \
                    started_at, completed_at, updated_at \
             FROM zyal_super_phases WHERE run_id = ?1 ORDER BY phase_id",
        )?;
        let phase_rows = phase_stmt
            .query_map([run_id], |row| {
                let phase_id: String = row.get(0)?;
                let depends_json: String = row.get(3)?;
                let depends_on = match serde_json::from_str::<Vec<String>>(&depends_json) {
                    Ok(depends) => depends,
                    Err(err) => {
                        eprintln!(
                            "port-run --status: phase {phase_id} has malformed depends_on JSON: {err}"
                        );
                        Vec::new()
                    }
                };
                Ok(PhaseStatusRow {
                    phase_id,
                    name: row.get(1)?,
                    objective: row.get(2)?,
                    depends_on,
                    status: row.get(4)?,
                    summary: row.get(5)?,
                    started_at: row.get(6)?,
                    completed_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut task_stmt = self.connection().prepare(
            "SELECT task_id, phase_id, title, status, owner, summary, updated_at \
             FROM zyal_super_tasks WHERE run_id = ?1 ORDER BY phase_id, task_id",
        )?;
        let task_rows = task_stmt
            .query_map([run_id], |row| {
                Ok(TaskStatusRow {
                    task_id: row.get(0)?,
                    phase_id: row.get(1)?,
                    title: row.get(2)?,
                    status: row.get(3)?,
                    owner: row.get(4)?,
                    summary: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(RunStatusReport {
            run_id: run_id.to_string(),
            phases: phase_rows,
            tasks: task_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ControllerPolicy, Gate, GateKind, MemoryPolicy, ParityPolicy, Phase, PhaseSignoffMode,
        PhaseStatus, RepoGraphPolicy, SandboxPolicy, WriteScope,
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
    fn manifest_for_run_roundtrips() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest, None).unwrap();

        let decoded = store
            .manifest_for_run(&run_id)
            .unwrap()
            .expect("manifest should exist");
        assert_eq!(decoded.id, manifest.id);
        assert_eq!(decoded.name, manifest.name);
        assert_eq!(decoded.phases.len(), manifest.phases.len());
    }

    #[test]
    fn status_report_returns_phase_and_task_rows() {
        let store = SupervisorStore::open_in_memory().unwrap();
        let manifest = sample_manifest();
        let run_id = store.init_run(&manifest, None).unwrap();

        store
            .record_phase_status(&run_id, "p00", PhaseStatus::Running, "kicked off")
            .unwrap();
        store
            .connection()
            .execute(
                r#"
                INSERT INTO zyal_super_tasks (
                    run_id, task_id, phase_id, title, status, owner, lease_until,
                    scope_json, summary, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, '{}', ?6, ?7, ?7)
                "#,
                rusqlite::params![
                    run_id,
                    "task-1",
                    "p00",
                    "Task 1",
                    "pending",
                    "queued",
                    "2026-06-12T00:00:00Z",
                ],
            )
            .unwrap();

        let report = store.status_report(&run_id).unwrap();
        assert_eq!(report.run_id, run_id);
        assert_eq!(report.phases.len(), manifest.phases.len());
        assert_eq!(report.tasks.len(), 1);
        assert_eq!(report.phases[0].phase_id, "p00");
        assert_eq!(report.phases[0].depends_on, Vec::<String>::new());
        assert_eq!(report.tasks[0].task_id, "task-1");
        assert_eq!(report.tasks[0].phase_id, "p00");
        assert_eq!(report.tasks[0].summary, "queued");
    }
}
