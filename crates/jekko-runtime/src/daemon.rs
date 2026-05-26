//! Daemon registry.
//!
//! Ported in skeleton form from the very large
//! `packages/jekko/src/session/daemon*.ts` family. The TS daemon system
//! tracks "runs", "iterations", "tasks", "workers", "artifacts", and a
//! handful of orthogonal state machines per run. Here we ship the data
//! shape and an in-memory registry; the full state machine still lives
//! in TS pending a focused port pass.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::bus::Bus;
use crate::error::{RuntimeError, RuntimeResult};

pub mod super_reasoning;

pub use super_reasoning::{SuperReasoningPlan, SUPER_REASONING_METADATA_KEY};

/// Lifecycle status of a daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonStatus {
    /// Newly registered, not yet running.
    Pending,
    /// Active run in flight.
    Running,
    /// Run paused awaiting input.
    Paused,
    /// Run terminated successfully.
    Done,
    /// Run terminated with an error.
    Failed,
}

/// Materialised daemon record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonRecord {
    /// Daemon id.
    pub id: String,
    /// Owning session id.
    pub session_id: String,
    /// Human-readable label.
    pub name: String,
    /// Current status.
    pub status: DaemonStatus,
    /// Creation timestamp (ms since epoch).
    pub time_created: i64,
    /// Last-update timestamp (ms since epoch).
    pub time_updated: i64,
    /// Free-form metadata.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// In-memory daemon registry.
#[derive(Debug, Default)]
pub struct DaemonRegistry {
    inner: RwLock<HashMap<String, DaemonRecord>>,
    bus: Option<Arc<Bus>>,
}

impl DaemonRegistry {
    /// Construct an empty registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Construct an empty registry that publishes daemon lifecycle events.
    pub fn with_bus(bus: Arc<Bus>) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(HashMap::new()),
            bus: Some(bus),
        })
    }

    /// Register a new daemon.
    pub async fn register(
        &self,
        session_id: impl Into<String>,
        name: impl Into<String>,
    ) -> DaemonRecord {
        let now = Utc::now().timestamp_millis();
        let record = DaemonRecord {
            id: format!("daemon_{}", Uuid::new_v4().simple()),
            session_id: session_id.into(),
            name: name.into(),
            status: DaemonStatus::Pending,
            time_created: now,
            time_updated: now,
            metadata: serde_json::json!({}),
        };
        self.inner
            .write()
            .await
            .insert(record.id.clone(), record.clone());
        self.publish_status(&record).await;
        record
    }

    /// Register a daemon and attach a validated super-reasoning mission plan.
    ///
    /// This is intentionally generic: the plan may describe a port, rewrite,
    /// research program, proof search, large refactor, or parity project. The
    /// host can derive it from existing ZYAL `workflow`, `fleet`, `memory`,
    /// `repo_intelligence`, `evidence`, `approvals`, and `sandbox` blocks
    /// without introducing target-specific top-level schema.
    pub async fn register_super_reasoning_plan(
        &self,
        session_id: impl Into<String>,
        name: impl Into<String>,
        plan: SuperReasoningPlan,
    ) -> RuntimeResult<DaemonRecord> {
        plan.validate()?;
        let record = self.register(session_id, name).await;
        self.attach_super_reasoning_plan(&record.id, plan).await?;
        self.get(&record.id)
            .await
            .ok_or_else(|| RuntimeError::not_found("daemon", &record.id))
    }

    /// Attach or replace the super-reasoning mission plan for an existing daemon.
    pub async fn attach_super_reasoning_plan(
        &self,
        id: &str,
        plan: SuperReasoningPlan,
    ) -> RuntimeResult<()> {
        plan.validate()?;
        let topological_phase_ids = plan.topological_phase_ids()?;
        let parallel_waves = plan.parallel_waves()?;
        let ready_phase_ids = plan.ready_phase_ids(std::iter::empty::<String>())?;
        let payload = serde_json::json!({
            "schema_version": super_reasoning::SUPER_REASONING_SCHEMA_VERSION,
            "plan": plan,
            "topological_phase_ids": topological_phase_ids,
            "parallel_waves": parallel_waves,
            "ready_phase_ids": ready_phase_ids,
        });

        let mut inner = self.inner.write().await;
        let rec = match inner.get_mut(id) {
            Some(r) => r,
            None => return Err(RuntimeError::not_found("daemon", id)),
        };
        if !rec.metadata.is_object() {
            rec.metadata = serde_json::json!({});
        }
        if let Some(obj) = rec.metadata.as_object_mut() {
            obj.insert(SUPER_REASONING_METADATA_KEY.to_string(), payload);
        }
        rec.time_updated = Utc::now().timestamp_millis();
        self.publish_status(rec).await;
        Ok(())
    }

    /// Return the typed super-reasoning plan attached to a daemon, if present.
    pub async fn super_reasoning_plan(
        &self,
        id: &str,
    ) -> RuntimeResult<Option<SuperReasoningPlan>> {
        let inner = self.inner.read().await;
        let rec = match inner.get(id) {
            Some(r) => r,
            None => return Err(RuntimeError::not_found("daemon", id)),
        };
        let Some(payload) = rec.metadata.get(SUPER_REASONING_METADATA_KEY) else {
            return Ok(None);
        };
        let Some(plan_value) = payload.get("plan") else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(plan_value.clone())?))
    }

    /// Compute currently-runnable phases from a daemon's super-reasoning plan.
    pub async fn super_reasoning_ready_phase_ids<I, S>(
        &self,
        id: &str,
        completed_phase_ids: I,
    ) -> RuntimeResult<Vec<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let plan = self
            .super_reasoning_plan(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found("super_reasoning_plan", id))?;
        plan.ready_phase_ids(completed_phase_ids)
    }

    /// Transition a daemon's status.
    pub async fn set_status(&self, id: &str, status: DaemonStatus) -> RuntimeResult<()> {
        let mut inner = self.inner.write().await;
        let rec = match inner.get_mut(id) {
            Some(r) => r,
            None => return Err(RuntimeError::not_found("daemon", id)),
        };
        rec.status = status;
        rec.time_updated = Utc::now().timestamp_millis();
        self.publish_status(rec).await;
        Ok(())
    }

    /// Fetch a daemon record.
    pub async fn get(&self, id: &str) -> Option<DaemonRecord> {
        self.inner.read().await.get(id).cloned()
    }

    /// List daemons for a given session.
    pub async fn list_for_session(&self, session_id: &str) -> Vec<DaemonRecord> {
        self.inner
            .read()
            .await
            .values()
            .filter(|d| d.session_id == session_id)
            .cloned()
            .collect()
    }

    async fn publish_status(&self, record: &DaemonRecord) {
        if let Some(bus) = &self.bus {
            let _ = bus
                .publish(
                    "daemon.status",
                    serde_json::to_value(record).unwrap_or_else(|_| serde_json::json!({})),
                )
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle() {
        let reg = DaemonRegistry::new();
        let rec = reg.register("session_1", "checker").await;
        assert_eq!(rec.status, DaemonStatus::Pending);
        reg.set_status(&rec.id, DaemonStatus::Running)
            .await
            .unwrap();
        assert_eq!(
            reg.get(&rec.id).await.unwrap().status,
            DaemonStatus::Running
        );
    }

    #[tokio::test]
    async fn list_for_session_filters() {
        let reg = DaemonRegistry::new();
        let _ = reg.register("session_1", "a").await;
        let _ = reg.register("session_1", "b").await;
        let _ = reg.register("session_2", "c").await;
        assert_eq!(reg.list_for_session("session_1").await.len(), 2);
        assert_eq!(reg.list_for_session("session_2").await.len(), 1);
    }

    #[tokio::test]
    async fn lifecycle_events_publish_when_bus_is_present() {
        let bus = Arc::new(Bus::new());
        let reg = DaemonRegistry::with_bus(bus.clone());
        let mut sub = bus.subscribe("daemon.status").await;

        let rec = reg.register("session_1", "checker").await;
        assert_eq!(sub.recv().await.unwrap().kind, "daemon.status");

        reg.set_status(&rec.id, DaemonStatus::Running)
            .await
            .unwrap();
        assert_eq!(sub.recv().await.unwrap().kind, "daemon.status");
    }

    #[tokio::test]
    async fn stores_super_reasoning_plan_metadata() {
        let reg = DaemonRegistry::new();
        let plan = SuperReasoningPlan::default_megaproject_plan(
            "mission_redis_like_rewrite",
            "Rewrite a reference system with full parity, performance closure, and final signoff.",
        );

        let rec = reg
            .register_super_reasoning_plan("session_1", "mega project", plan)
            .await
            .unwrap();

        assert!(rec.metadata.get(SUPER_REASONING_METADATA_KEY).is_some());
        let ready = reg
            .super_reasoning_ready_phase_ids(&rec.id, std::iter::empty::<String>())
            .await
            .unwrap();
        assert_eq!(ready, vec!["source_of_truth"]);
    }
}
