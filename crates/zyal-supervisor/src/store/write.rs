//! Write-side queries for [`SupervisorStore`].
//!
//! All mutating operations on the supervisor store live here: seeding a
//! run from a manifest, updating phase status, recording sign-offs, and
//! appending memory / evidence rows. The parent module owns the
//! connection lifecycle and read-side queries.

use chrono::Utc;
use rusqlite::params;

use super::SupervisorStore;
use crate::model::{PhaseStatus, SuperWorkflow};

impl SupervisorStore {
    /// Initialize a run from a manifest. Returns the assigned `run_id`.
    ///
    /// When `requested_id` is `Some`, that value is used verbatim. Otherwise
    /// the run id is derived from the manifest id plus a millisecond timestamp
    /// so multiple runs of the same workflow coexist. All phases are seeded
    /// as [`PhaseStatus::Pending`]; callers can promote dependency-free phases
    /// to `Ready` via [`Self::record_phase_status`] or by reading
    /// [`crate::planner::ready_phases`].
    pub fn init_run(
        &self,
        manifest: &SuperWorkflow,
        requested_id: Option<&str>,
    ) -> rusqlite::Result<String> {
        let now_dt = Utc::now();
        let now = now_dt.to_rfc3339();
        let run_id = requested_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}-{}", manifest.id, now_dt.timestamp_millis()));
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

    /// Reset in-flight running phases to pending during resume.
    pub fn reset_running_phases(&self, run_id: &str) -> rusqlite::Result<usize> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"
            UPDATE zyal_super_phases
            SET status = ?1,
                updated_at = ?2
            WHERE run_id = ?3 AND status = ?4
            "#,
            params![
                PhaseStatus::Pending.as_str(),
                now,
                run_id,
                PhaseStatus::Running.as_str(),
            ],
        )
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
