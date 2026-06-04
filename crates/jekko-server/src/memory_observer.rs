//! Background observer that ingests live session messages into memory (W1).
//!
//! Subscribes to the `session.message.appended` bus topic and `observe`s each
//! message into the cogcore-backed [`MemoryService`] off the turn hot path. No
//! runner is involved; this only reads the bus and writes memory.

use std::sync::{Arc, Mutex};

use jekko_memory::{MemoryConfig, MemoryError, MemoryService, TurnObservation};
use jekko_runtime::bus::Bus;
use jekko_runtime::session::MessageInfo;
use jekko_store::db::Db;
use jekko_store::memory_wal::SqliteWalSink;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

/// The cogcore-backed memory service shared between the observer task and the
/// host. SQLite-backed, so it is wrapped in a `Mutex` for cross-task access.
pub type SharedMemory = Arc<Mutex<MemoryService<SqliteWalSink>>>;

/// Bus topic the observer listens on.
pub const SESSION_MESSAGE_APPENDED: &str = "session.message.appended";

/// Subscribe to `session.message.appended` and spawn a background task that
/// `observe`s each message into `memory`. The subscription is established before
/// returning, so callers may publish immediately after this resolves without
/// missing events (the broadcast bus is otherwise lossy).
pub async fn spawn_memory_observer(bus: Arc<Bus>, memory: SharedMemory) -> JoinHandle<()> {
    let mut rx = bus.subscribe(SESSION_MESSAGE_APPENDED).await;
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    if let Ok(msg) = serde_json::from_value::<MessageInfo>(env.properties) {
                        let turn = message_info_to_turn(&msg);
                        if let Ok(mut guard) = memory.lock() {
                            // Observe off the hot path. Persistence errors surface
                            // through the WAL sink; they are not fatal to the turn.
                            let _ = guard.observe_turn(turn);
                        }
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    })
}

/// Open a SQLite-backed memory service for `db_path`, restore prior memory from
/// the WAL, and spawn the observe subscriber. Returns the shared handle to hand
/// to [`crate::state::AppState::with_memory`]. `db_path` may be a file path or
/// `":memory:"`. This is the single activation entry point the server bootstrap
/// (`serve()`) calls to bring cogcore memory live.
pub async fn activate_memory(db_path: &str, bus: Arc<Bus>) -> Result<SharedMemory, MemoryError> {
    let db = Db::open(db_path).map_err(|e| MemoryError::Sink(e.to_string()))?;
    let sink = SqliteWalSink::new(db, "global");
    let mut service = MemoryService::new(sink, MemoryConfig::default());
    service.bootstrap()?;
    let memory: SharedMemory = Arc::new(Mutex::new(service));
    spawn_memory_observer(bus, memory.clone()).await;
    Ok(memory)
}

fn message_info_to_turn(msg: &MessageInfo) -> TurnObservation {
    TurnObservation::new(
        msg.id.as_str(),
        msg.session_id.as_str(),
        msg.role.as_str(),
        extract_body(&msg.data),
        msg.time_created,
    )
}

fn extract_body(payload: &serde_json::Value) -> String {
    if let Some(s) = payload.as_str() {
        return s.to_string();
    }
    for key in ["summary", "text", "body", "message", "title", "content"] {
        if let Some(s) = payload.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    serde_json::to_string(payload).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jekko_memory::MemoryConfig;
    use std::time::{Duration, Instant};

    // `MessageId`/`SessionId` are private to jekko-runtime, so build via the
    // public `Deserialize` surface (the same JSON shape the bus carries).
    fn message(role: &str, body: &str) -> MessageInfo {
        serde_json::from_value(serde_json::json!({
            "id": format!("msg_{role}"),
            "session_id": "sess_7",
            "role": role,
            "data": body,
            "time_created": 1_700_000_000_000i64,
            "time_updated": 1_700_000_000_000i64,
        }))
        .expect("MessageInfo from json")
    }

    #[tokio::test]
    async fn observer_ingests_published_messages() {
        let bus = Arc::new(Bus::new());
        let sink = SqliteWalSink::open_in_memory("global").expect("open db");
        let memory: SharedMemory = Arc::new(Mutex::new(MemoryService::new(
            sink,
            MemoryConfig::default(),
        )));
        let _handle = spawn_memory_observer(bus.clone(), memory.clone()).await;

        bus.publish(
            SESSION_MESSAGE_APPENDED,
            serde_json::to_value(message("user", "neutrinos have mass")).unwrap(),
        )
        .await;

        // Robust wait: poll until the observer ingests (no fixed sleep / no flake).
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if memory.lock().unwrap().core().wal_len() > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "observer did not ingest the published message"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let mut guard = memory.lock().unwrap();
        assert!(guard.recall_block("neutrinos", 512).is_some());
    }

    #[tokio::test]
    async fn activate_memory_brings_observe_live() {
        let bus = Arc::new(Bus::new());
        let memory = activate_memory(":memory:", bus.clone())
            .await
            .expect("activate memory");
        bus.publish(
            SESSION_MESSAGE_APPENDED,
            serde_json::to_value(message("user", "muons are heavier")).unwrap(),
        )
        .await;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if memory.lock().unwrap().core().wal_len() > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "activate_memory observer did not ingest"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut guard = memory.lock().unwrap();
        assert!(guard.recall_block("muons", 512).is_some());
    }

    #[test]
    fn appstate_recall_block_for_is_none_without_memory() {
        let state = crate::state::AppState::new();
        assert!(state.recall_block_for("anything", 256).is_none());
    }

    #[tokio::test]
    async fn appstate_recall_block_for_surfaces_observed_memory() {
        use crate::state::AppState;
        let state = AppState::new();
        let mem = activate_memory(":memory:", state.bus.clone())
            .await
            .expect("activate memory");
        let state = state.with_memory(mem);

        state
            .bus
            .publish(
                SESSION_MESSAGE_APPENDED,
                serde_json::to_value(message("user", "photons are massless")).unwrap(),
            )
            .await;

        // Full server-side path: activate -> observe via bus -> AppState recall.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if state.recall_block_for("photons", 512).is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "AppState::recall_block_for did not surface the observed memory"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
