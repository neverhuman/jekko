//! Shared application state passed to every Axum handler.
//!
//! The state bundles long-lived runtime services: the SQLite store, the
//! event bus, the session/permission/status services, and the auth + CORS
//! configuration. All inner handles are `Arc`-wrapped so cloning [`AppState`]
//! is cheap and the runtime can be shared across worker threads.

use std::collections::BTreeMap;
use std::sync::Arc;

use jekko_core::config::Config;
use jekko_runtime::bus::Bus;
use jekko_runtime::permission::PermissionService;
use jekko_runtime::session::SessionService;
use jekko_runtime::status::StatusService;
use tokio::sync::RwLock;

use crate::auth::AuthConfig;
use crate::cors::CorsConfig;
use crate::memory_observer::SharedMemory;

/// Default working directory used when the OS denies `current_dir()`.
const ROOT_DIR_DEFAULT: &str = "/";

/// Workspace metadata projected through `/api/v1/workspace`.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceRegistry {
    /// Workspace id -> JSON descriptor.
    pub entries: BTreeMap<String, serde_json::Value>,
}

/// Daemon run registry. Mirrors the TS `Daemon.Service.list`/`get` shape
/// without coupling to a runtime trait that has not landed yet.
#[derive(Clone, Debug, Default)]
pub struct DaemonRegistry {
    /// Run id -> JSON descriptor.
    pub runs: BTreeMap<String, serde_json::Value>,
}

/// Interactive question queue. Each pending question is keyed by id.
#[derive(Clone, Debug, Default)]
pub struct QuestionRegistry {
    /// Question id -> descriptor.
    pub pending: BTreeMap<String, serde_json::Value>,
}

/// Top-level shared state. Cheap to clone (every field is `Arc`-wrapped).
#[derive(Clone)]
pub struct AppState {
    /// Event bus.
    pub bus: Arc<Bus>,
    /// Session service.
    pub sessions: Arc<SessionService>,
    /// Permission service.
    pub permissions: Arc<PermissionService>,
    /// Status service.
    pub status: Arc<StatusService>,
    /// Active in-memory config (matches the on-disk `jekko.json`).
    pub config: Arc<RwLock<Config>>,
    /// Workspace registry.
    pub workspaces: Arc<RwLock<WorkspaceRegistry>>,
    /// Daemon registry.
    pub daemons: Arc<RwLock<DaemonRegistry>>,
    /// Question registry.
    pub questions: Arc<RwLock<QuestionRegistry>>,
    /// Auth configuration.
    pub auth: Arc<AuthConfig>,
    /// CORS configuration.
    pub cors: Arc<CorsConfig>,
    /// Instance metadata (paths, directory).
    pub instance: Arc<InstanceMeta>,
    /// Feature-flag toggle map (mirrors the experimental section of config).
    pub experimental: Arc<RwLock<BTreeMap<String, serde_json::Value>>>,
    /// Live cogcore-backed memory service (W1). `None` until activated at
    /// `serve()` via [`crate::memory_observer::activate_memory`]; the observe
    /// subscriber and recall path read it. Optional so `AppState::new()` and
    /// tests stay broker/DB-free.
    pub memory: Option<SharedMemory>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("auth_required", &self.auth.required())
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

/// Static instance metadata exposed by `/api/v1/instance`.
#[derive(Clone, Debug)]
pub struct InstanceMeta {
    /// Process working directory.
    pub directory: String,
    /// User home directory.
    pub home: String,
    /// Configuration directory.
    pub config_dir: String,
    /// Runtime state directory.
    pub state_dir: String,
    /// Optional worktree path (Git, dev mode).
    pub worktree: Option<String>,
    /// Instance id.
    pub instance_id: String,
}

impl Default for InstanceMeta {
    fn default() -> Self {
        let directory = match std::env::current_dir() {
            Ok(p) => p.display().to_string(),
            Err(_) => ROOT_DIR_DEFAULT.to_string(),
        };
        // Explicit typed branching: an empty `HOME` propagates to empty
        // `config_dir` / `state_dir` so downstream paths refuse to write.
        #[allow(clippy::manual_unwrap_or_default)]
        let home: String = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => String::new(),
        };
        let config_dir = if home.is_empty() {
            String::new()
        } else {
            format!("{home}/.config/jekko")
        };
        let state_dir = if home.is_empty() {
            String::new()
        } else {
            format!("{home}/.local/state/jekko")
        };
        Self {
            directory,
            home,
            config_dir,
            state_dir,
            worktree: None,
            instance_id: format!("inst_{}", uuid::Uuid::new_v4().simple()),
        }
    }
}

impl AppState {
    /// Construct an `AppState` with the supplied services and default
    /// configuration. Useful for unit tests.
    pub fn new() -> Self {
        let bus = Arc::new(Bus::new());
        let permissions = Arc::new(PermissionService::new(bus.clone()));
        let status = Arc::new(StatusService::new(bus.clone()));
        let sessions = Arc::new(SessionService::new());
        Self {
            bus,
            sessions,
            permissions,
            status,
            config: Arc::new(RwLock::new(Config::default())),
            workspaces: Arc::new(RwLock::new(WorkspaceRegistry::default())),
            daemons: Arc::new(RwLock::new(DaemonRegistry::default())),
            questions: Arc::new(RwLock::new(QuestionRegistry::default())),
            auth: Arc::new(AuthConfig::default()),
            cors: Arc::new(CorsConfig::default()),
            instance: Arc::new(InstanceMeta::default()),
            experimental: Arc::new(RwLock::new(BTreeMap::new())),
            memory: None,
        }
    }

    /// Set the auth config (builder style).
    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = Arc::new(auth);
        self
    }

    /// Set the CORS config (builder style).
    pub fn with_cors(mut self, cors: CorsConfig) -> Self {
        self.cors = Arc::new(cors);
        self
    }

    /// Set instance metadata (builder style).
    pub fn with_instance(mut self, instance: InstanceMeta) -> Self {
        self.instance = Arc::new(instance);
        self
    }

    /// Set the initial config (builder style).
    pub fn with_config(self, config: Config) -> Self {
        if let Ok(mut guard) = self.config.try_write() {
            *guard = config;
        }
        self
    }

    /// Attach the live cogcore-backed memory service (builder style). Set at
    /// `serve()` after [`crate::memory_observer::activate_memory`] opens the DB,
    /// restores the WAL, and spawns the observe subscriber.
    pub fn with_memory(mut self, memory: SharedMemory) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Server-side recall helper for future HTTP/session paths. Returns `None`
    /// when memory is inactive or has nothing relevant. Jekko owns this context;
    /// it never drives a runner.
    pub fn recall_block_for(&self, text: &str, token_budget: u32) -> Option<String> {
        let memory = self.memory.as_ref()?;
        let mut guard = memory.lock().ok()?;
        guard.recall_block(text, token_budget)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
