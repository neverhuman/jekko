//! Structured, idempotency-keyed daemon orchestration events.
//!
//! This module defines a small, generic event vocabulary that external
//! orchestrators (not coupled to any specific host) can consume to build a
//! replayable run/task feed. Each event carries a stable `kind`, the owning
//! `run_id`, the relevant entity ids, a millisecond timestamp, and a
//! deterministic `idempotency_key` so consumers can de-duplicate replays.
//!
//! Events are published onto the existing [`crate::bus::Bus`]; the durable
//! `daemon_event` table already persists arbitrary JSON payloads, so the
//! `idempotency_key` is embedded inside the payload (no schema migration is
//! required). Read-only HTTP consumers re-project the key out of the payload.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bus::{Bus, EventEnvelope};

/// `daemon.run.created` — a new run was registered.
pub const RUN_CREATED: &str = "daemon.run.created";
/// `daemon.run.status_changed` — a run transitioned status.
pub const RUN_STATUS_CHANGED: &str = "daemon.run.status_changed";
/// `daemon.run.stopped` — a run reached a terminal state.
pub const RUN_STOPPED: &str = "daemon.run.stopped";
/// `daemon.task.created` — a task was created within a run.
pub const TASK_CREATED: &str = "daemon.task.created";
/// `daemon.task.status_changed` — a task transitioned status.
pub const TASK_STATUS_CHANGED: &str = "daemon.task.status_changed";
/// `daemon.artifact.stored` — an artifact was persisted for a run/task.
pub const ARTIFACT_STORED: &str = "daemon.artifact.stored";

/// Property key under which the deterministic idempotency key is published.
pub const IDEMPOTENCY_KEY_FIELD: &str = "idempotency_key";

/// Compute a deterministic idempotency key for an event.
///
/// The key is a stable SHA-256 over the event `kind`, `run_id`, the ordered
/// entity ids, and the `timestamp_ms`. Two publications describing the same
/// logical transition therefore produce the same key, which lets consumers
/// collapse replays without coordinating with the producer.
pub fn idempotency_key(kind: &str, run_id: &str, ids: &[&str], timestamp_ms: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0u8]);
    hasher.update(run_id.as_bytes());
    for id in ids {
        hasher.update([0u8]);
        hasher.update(id.as_bytes());
    }
    hasher.update([0u8]);
    hasher.update(timestamp_ms.to_le_bytes());
    let digest = hasher.finalize();
    format!("idk_{digest:x}")
}

/// A typed daemon orchestration event ready to be published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonEvent {
    /// Event kind (one of the `daemon.*` constants in this module).
    #[serde(rename = "type")]
    pub kind: String,
    /// Owning run id.
    pub run_id: String,
    /// Related entity ids (e.g. `[task_id]`, `[artifact_id, task_id]`).
    #[serde(default)]
    pub ids: Vec<String>,
    /// Event timestamp (ms since epoch).
    pub timestamp_ms: i64,
    /// Deterministic idempotency key.
    pub idempotency_key: String,
    /// Free-form extra fields merged into the published payload.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl DaemonEvent {
    /// Build a typed event, computing the idempotency key from its identity.
    pub fn new(
        kind: &'static str,
        run_id: impl Into<String>,
        ids: Vec<String>,
        timestamp_ms: i64,
    ) -> Self {
        let run_id = run_id.into();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let idempotency_key = idempotency_key(kind, &run_id, &id_refs, timestamp_ms);
        Self {
            kind: kind.to_string(),
            run_id,
            ids,
            timestamp_ms,
            idempotency_key,
            extra: serde_json::Map::new(),
        }
    }

    /// Attach an extra payload field (builder style).
    #[must_use]
    pub fn with_field(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Render the event as the JSON payload published onto the bus.
    pub fn to_properties(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("run_id".into(), serde_json::Value::String(self.run_id.clone()));
        map.insert(
            "ids".into(),
            serde_json::Value::Array(
                self.ids
                    .iter()
                    .map(|id| serde_json::Value::String(id.clone()))
                    .collect(),
            ),
        );
        map.insert(
            "timestamp_ms".into(),
            serde_json::Value::Number(self.timestamp_ms.into()),
        );
        map.insert(
            IDEMPOTENCY_KEY_FIELD.into(),
            serde_json::Value::String(self.idempotency_key.clone()),
        );
        for (key, value) in &self.extra {
            map.entry(key.clone()).or_insert_with(|| value.clone());
        }
        serde_json::Value::Object(map)
    }
}

/// Publishes structured daemon events onto a [`Bus`].
///
/// This is intentionally generic and host-agnostic: any orchestration layer
/// can call these helpers to emit a consistent, replayable feed without
/// depending on a specific run/task implementation.
#[derive(Clone)]
pub struct DaemonEventPublisher {
    bus: Arc<Bus>,
}

impl std::fmt::Debug for DaemonEventPublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonEventPublisher").finish_non_exhaustive()
    }
}

impl DaemonEventPublisher {
    /// Construct a publisher over the supplied bus.
    pub fn new(bus: Arc<Bus>) -> Self {
        Self { bus }
    }

    /// Publish a pre-built [`DaemonEvent`], returning the published envelope.
    pub async fn publish(&self, event: &DaemonEvent) -> EventEnvelope {
        self.bus
            .publish(&event.kind.clone(), event.to_properties())
            .await
    }

    /// Publish `daemon.run.created`.
    pub async fn run_created(&self, run_id: &str, timestamp_ms: i64) -> EventEnvelope {
        self.publish(&DaemonEvent::new(RUN_CREATED, run_id, vec![], timestamp_ms))
            .await
    }

    /// Publish `daemon.run.status_changed`.
    pub async fn run_status_changed(
        &self,
        run_id: &str,
        status: &str,
        timestamp_ms: i64,
    ) -> EventEnvelope {
        self.publish(
            &DaemonEvent::new(RUN_STATUS_CHANGED, run_id, vec![], timestamp_ms)
                .with_field("status", serde_json::Value::String(status.to_string())),
        )
        .await
    }

    /// Publish `daemon.run.stopped`.
    pub async fn run_stopped(
        &self,
        run_id: &str,
        reason: Option<&str>,
        timestamp_ms: i64,
    ) -> EventEnvelope {
        let mut event = DaemonEvent::new(RUN_STOPPED, run_id, vec![], timestamp_ms);
        if let Some(reason) = reason {
            event = event.with_field("reason", serde_json::Value::String(reason.to_string()));
        }
        self.publish(&event).await
    }

    /// Publish `daemon.task.created`.
    pub async fn task_created(
        &self,
        run_id: &str,
        task_id: &str,
        timestamp_ms: i64,
    ) -> EventEnvelope {
        self.publish(&DaemonEvent::new(
            TASK_CREATED,
            run_id,
            vec![task_id.to_string()],
            timestamp_ms,
        ))
        .await
    }

    /// Publish `daemon.task.status_changed`.
    pub async fn task_status_changed(
        &self,
        run_id: &str,
        task_id: &str,
        status: &str,
        timestamp_ms: i64,
    ) -> EventEnvelope {
        self.publish(
            &DaemonEvent::new(
                TASK_STATUS_CHANGED,
                run_id,
                vec![task_id.to_string()],
                timestamp_ms,
            )
            .with_field("status", serde_json::Value::String(status.to_string())),
        )
        .await
    }

    /// Publish `daemon.artifact.stored`.
    pub async fn artifact_stored(
        &self,
        run_id: &str,
        artifact_id: &str,
        task_id: Option<&str>,
        timestamp_ms: i64,
    ) -> EventEnvelope {
        let mut ids = vec![artifact_id.to_string()];
        if let Some(task_id) = task_id {
            ids.push(task_id.to_string());
        }
        self.publish(&DaemonEvent::new(ARTIFACT_STORED, run_id, ids, timestamp_ms))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_key_is_deterministic_and_identity_sensitive() {
        let a = idempotency_key(RUN_CREATED, "run-1", &[], 100);
        let b = idempotency_key(RUN_CREATED, "run-1", &[], 100);
        assert_eq!(a, b);
        assert!(a.starts_with("idk_"));
        assert_ne!(a, idempotency_key(RUN_CREATED, "run-1", &[], 101));
        assert_ne!(a, idempotency_key(RUN_STOPPED, "run-1", &[], 100));
        assert_ne!(a, idempotency_key(RUN_CREATED, "run-2", &[], 100));
    }

    #[test]
    fn event_properties_carry_envelope_fields() {
        let event = DaemonEvent::new(TASK_CREATED, "run-1", vec!["task-1".into()], 42)
            .with_field("title", serde_json::json!("seed"));
        let props = event.to_properties();
        assert_eq!(props["run_id"], "run-1");
        assert_eq!(props["ids"][0], "task-1");
        assert_eq!(props["timestamp_ms"], 42);
        assert_eq!(props["title"], "seed");
        assert_eq!(props[IDEMPOTENCY_KEY_FIELD], event.idempotency_key);
    }

    #[tokio::test]
    async fn publisher_emits_all_six_kinds() {
        let bus = Arc::new(Bus::new());
        let mut sub = bus.subscribe_all();
        let publisher = DaemonEventPublisher::new(bus.clone());

        publisher.run_created("run-1", 1).await;
        publisher.run_status_changed("run-1", "running", 2).await;
        publisher.task_created("run-1", "task-1", 3).await;
        publisher
            .task_status_changed("run-1", "task-1", "done", 4)
            .await;
        publisher
            .artifact_stored("run-1", "art-1", Some("task-1"), 5)
            .await;
        publisher.run_stopped("run-1", Some("complete"), 6).await;

        let mut kinds = Vec::new();
        for _ in 0..6 {
            let env = sub.recv().await.unwrap();
            assert!(env.properties[IDEMPOTENCY_KEY_FIELD].is_string());
            assert_eq!(env.properties["run_id"], "run-1");
            kinds.push(env.kind);
        }
        assert_eq!(
            kinds,
            vec![
                RUN_CREATED,
                RUN_STATUS_CHANGED,
                TASK_CREATED,
                TASK_STATUS_CHANGED,
                ARTIFACT_STORED,
                RUN_STOPPED,
            ]
        );
    }
}
