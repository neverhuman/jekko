//! Domain model for ZYAL SuperWorkflow manifests.
//!
//! The shapes here are the in-process representation handed to the planner
//! and the SQLite store. They are intentionally minimal and stable; richer
//! per-host extensions can be carried as JSON in evidence / memory rows.

use serde::{Deserialize, Serialize};

/// Top-level SuperWorkflow manifest.
///
/// A SuperWorkflow is a long-running, dependency-driven job composed of
/// 9..=12 phases (validated by [`crate::planner::validate_manifest`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SuperWorkflow {
    /// Stable workflow id (used as run-id prefix by the store).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Objective text.
    pub objective: String,
    /// Macro phases. Must contain between 9 and 12 phases inclusive.
    pub phases: Vec<Phase>,
    /// Controller orchestration policy.
    #[serde(default)]
    pub controller: ControllerPolicy,
    /// Active memory policy.
    #[serde(default)]
    pub memory: MemoryPolicy,
    /// Persistent sandbox policy.
    #[serde(default)]
    pub sandbox: SandboxPolicy,
    /// Repo graph indexing policy.
    #[serde(default)]
    pub repo_graph: RepoGraphPolicy,
    /// Parity / differential testing policy.
    #[serde(default)]
    pub parity: ParityPolicy,
}

/// A macro phase in the SuperWorkflow DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Phase {
    /// Stable phase id (unique within a workflow).
    pub id: String,
    /// Human-readable phase name.
    pub name: String,
    /// Objective text.
    pub objective: String,
    /// Phase ids that must reach [`PhaseStatus::Complete`] before this phase
    /// can transition to [`PhaseStatus::Ready`].
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// What this phase is allowed to write to.
    #[serde(default)]
    pub write_scope: WriteScope,
    /// Required sign-off mode before this phase can complete.
    #[serde(default)]
    pub signoff: PhaseSignoffMode,
    /// Exit gates this phase must satisfy.
    #[serde(default)]
    pub gates: Vec<Gate>,
}

/// Persisted phase status.
///
/// Default is [`PhaseStatus::Pending`]. The store seeds every phase as
/// `Pending` on `init_run`, then promotes dependency-satisfied phases to
/// `Ready` via [`crate::planner::ready_phases`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    /// Phase is not ready because dependencies remain.
    #[default]
    Pending,
    /// Phase dependencies are complete and it can be scheduled.
    Ready,
    /// Phase is actively running.
    Running,
    /// Phase is blocked (out-of-band signal).
    Blocked,
    /// Phase awaits sign-off / review.
    Review,
    /// Phase completed all gates and sign-offs.
    Complete,
    /// Phase was parked with summary and follow-up tasks.
    Parked,
    /// Phase failed irrecoverably.
    Failed,
}

impl PhaseStatus {
    /// Stable storage string.
    pub fn as_str(self) -> &'static str {
        match self {
            PhaseStatus::Pending => "pending",
            PhaseStatus::Ready => "ready",
            PhaseStatus::Running => "running",
            PhaseStatus::Blocked => "blocked",
            PhaseStatus::Review => "review",
            PhaseStatus::Complete => "complete",
            PhaseStatus::Parked => "parked",
            PhaseStatus::Failed => "failed",
        }
    }

    /// Parse from storage string.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => PhaseStatus::Pending,
            "ready" => PhaseStatus::Ready,
            "running" => PhaseStatus::Running,
            "blocked" => PhaseStatus::Blocked,
            "review" => PhaseStatus::Review,
            "complete" => PhaseStatus::Complete,
            "parked" => PhaseStatus::Parked,
            "failed" => PhaseStatus::Failed,
            _ => return None,
        })
    }
}

/// A discrete unit of work materialized under a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Task {
    /// Stable task id.
    pub id: String,
    /// Owning phase id.
    pub phase_id: String,
    /// Human-readable title.
    pub title: String,
    /// Lifecycle status.
    pub status: TaskStatus,
    /// Optional lease owner.
    #[serde(default)]
    pub owner: Option<String>,
    /// Optional lease expiry (epoch seconds).
    #[serde(default)]
    pub lease_until: Option<i64>,
}

/// Persisted task status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is available for lease.
    #[default]
    Pending,
    /// Task is leased to an agent and executing.
    Running,
    /// Task completed successfully.
    Done,
    /// Task is blocked.
    Blocked,
}

impl TaskStatus {
    /// Stable storage string.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
        }
    }

    /// Parse from storage string.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => TaskStatus::Pending,
            "running" => TaskStatus::Running,
            "done" => TaskStatus::Done,
            "blocked" => TaskStatus::Blocked,
            _ => return None,
        })
    }
}

/// Exit gate definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Gate {
    /// Gate name (stable, unique within a phase).
    pub name: String,
    /// Gate kind / semantics.
    pub kind: GateKind,
    /// Whether the gate is required to close the phase.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

/// Gate categories recognized by the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// Tests must be green (e.g. `cargo test`).
    TestsGreen,
    /// Parity suite must pass.
    ParitySuite,
    /// Evidence bundle must be present and complete.
    EvidenceBundle,
    /// Plan receipt must be recorded.
    PlanReceipt,
    /// Repo graph index must be fresh.
    RepoGraphFresh,
    /// Host-defined custom gate.
    Custom,
}

/// Phase-scoped write authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteScope {
    /// Writes are limited to scratch (no merge target).
    ScratchOnly,
    /// Writes go to an isolated worktree.
    #[default]
    IsolatedWorktree,
    /// Writes target an integration branch awaiting sign-off.
    IntegrationBranch,
    /// Writes apply directly to the primary repo (highest authority).
    PrimaryRepo,
}

/// Required sign-off mode before a phase can complete.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseSignoffMode {
    /// No sign-off required.
    None,
    /// A single reviewer must approve.
    #[default]
    Single,
    /// Multiple reviewers must approve (quorum policy is host-defined).
    Quorum,
}

/// Controller orchestration policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ControllerPolicy {
    /// Maximum ready phases allowed to run concurrently.
    #[serde(default)]
    pub max_parallel_phases: Option<u32>,
    /// Maximum workers per phase allowed to run concurrently.
    #[serde(default)]
    pub max_parallel_workers_per_phase: Option<u32>,
}

/// Active memory policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryPolicy {
    /// Whether compounding (cross-phase distillation) is enabled.
    #[serde(default)]
    pub compounding_enabled: bool,
    /// Retention in days for transient memory rows.
    #[serde(default)]
    pub retention_days: Option<u32>,
    /// Whether to preserve negative lessons.
    #[serde(default)]
    pub negative_memory: bool,
    /// Free-form memory class tags injected at phase start.
    #[serde(default)]
    pub inject_at_phase_start: Vec<String>,
}

/// Sandbox policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SandboxPolicy {
    /// Sandbox isolation mode.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Network egress policy.
    #[serde(default)]
    pub network: NetworkPolicy,
}

/// Sandbox isolation backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// In-process (no isolation; tests only).
    InProcess,
    /// Isolated git worktree.
    #[default]
    Worktree,
    /// Containerized sandbox.
    Container,
}

/// Network egress policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    /// No outbound network.
    #[default]
    Deny,
    /// Allowlisted egress only.
    Allowlist,
    /// Unrestricted egress (highest authority).
    Allow,
}

/// Repo graph indexing policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepoGraphPolicy {
    /// Whether to refresh the graph when a phase completes.
    #[serde(default)]
    pub refresh_on_phase_complete: bool,
    /// Backing store for the graph.
    #[serde(default)]
    pub store: GraphStore,
}

/// Repo graph storage backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphStore {
    /// Persist to the supervisor SQLite store.
    #[default]
    Sqlite,
    /// Keep in memory only.
    InMemory,
}

/// Parity / differential testing policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ParityPolicy {
    /// Whether parity execution runs in memory (ramdisk-like).
    #[serde(default)]
    pub in_memory: bool,
    /// Optional ramdisk mount root for parity runs.
    #[serde(default)]
    pub ramdisk_root: Option<String>,
}
