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
    /// How this phase's per-phase work is executed in `--live` mode.
    ///
    /// Absent (the default) preserves the historical behavior of delegating
    /// the phase to `jekko run --agent plan`. A present value lets a workflow
    /// route the phase to a named agent or a host-defined executor (e.g. an
    /// allowlisted SSH command) without changing the phase DAG shape.
    #[serde(default)]
    pub exec: Option<PhaseExec>,
}

/// Per-phase executor selection for `--live` walks.
///
/// This is an additive, backward-compatible extension to [`Phase`]: manifests
/// that omit `exec` deserialize to `None` and keep the default agent path.
// `Eq` is intentionally omitted: the `Jailgun` variant carries a
// `serde_json::Value` (not `Eq`). `PartialEq` is retained for tests and
// `Phase` itself does not derive `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhaseExec {
    /// Delegate the phase to a named jekko agent (`jekko run --agent <name>`).
    /// `name = "plan"` is equivalent to the historical default.
    Agent {
        /// Agent name passed to `jekko run --agent`.
        name: String,
    },
    /// Run an allowlisted command on a remote host over SSH and capture its
    /// stdout as the phase summary. Hosts are gated by the host runtime's
    /// allowlist (see the `--live` walker); this variant only carries intent.
    Ssh(SshExec),
    /// Run a Jailgun agent run (authenticated browser tabs -> tar.gz ->
    /// deploy/CI) as this phase, capturing the run summary. The host runtime
    /// invokes the `jailgun run-agent` interface (CLI now, HTTP later) and maps
    /// the resulting `JailgunAgentRunSummary` into the phase summary.
    Jailgun(JailgunExec),
}

/// Parameters for an [`PhaseExec::Ssh`] phase executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SshExec {
    /// SSH destination — an `ssh` argument such as `user@host` or a
    /// `~/.ssh/config` host alias.
    pub host: String,
    /// Command to execute on the remote host.
    pub command: String,
    /// When true, a non-zero remote exit code still completes the phase
    /// (stdout is captured as the summary). Defaults to false, where a
    /// non-zero exit fails the phase.
    #[serde(default)]
    pub allow_nonzero_exit: bool,
}

/// Parameters for a [`PhaseExec::Jailgun`] phase executor.
///
/// Carries the inputs needed to build a Jailgun agent run request. The host
/// runtime writes `prompt` to a temporary prompt file, builds a
/// `JailgunAgentRunRequest` (deep-merging `request_overrides`), and invokes the
/// `jailgun run-agent` interface. `prompt_ref` is the durable, prompt-text-free
/// reference recorded in summaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JailgunExec {
    /// Durable, prompt-text-free reference for the run (e.g. a workflow URI).
    pub prompt_ref: String,
    /// Prompt text; written to a temporary prompt file for the run, never
    /// echoed into durable phase summaries.
    pub prompt: String,
    /// Number of parallel tabs (candidate solutions). Bounded by Jailgun's own
    /// `JAILGUN_AGENT_MAX_TABS`; `None` defers to Jailgun's configured default.
    #[serde(default)]
    pub tabs: Option<u16>,
    /// Optional per-run wall-clock cap in seconds (bounded by Jailgun's cap).
    #[serde(default)]
    pub max_runtime_seconds: Option<u64>,
    /// Additional `JailgunAgentRunRequest` fields (repo / source_archive /
    /// deploy / ci / github / ...) deep-merged into the generated request JSON.
    /// Defaults to an empty object.
    #[serde(default)]
    pub request_overrides: serde_json::Value,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_without_exec_defaults_to_none() {
        // Manifests authored before `exec` existed must still parse, and must
        // keep the historical default-agent path (exec == None).
        let phase: Phase = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "Frame",
            "objective": "establish the contract",
        }))
        .expect("legacy phase without exec parses");
        assert_eq!(phase.exec, None);
        assert!(phase.depends_on.is_empty());
    }

    #[test]
    fn phase_exec_agent_round_trips() {
        let phase: Phase = serde_json::from_value(serde_json::json!({
            "id": "p2",
            "name": "Review",
            "objective": "critique the candidate",
            "exec": { "kind": "agent", "name": "code-reviewer" },
        }))
        .expect("agent exec parses");
        assert_eq!(
            phase.exec,
            Some(PhaseExec::Agent {
                name: "code-reviewer".to_string()
            })
        );
        // Round-trip through JSON preserves the tagged shape.
        let json = serde_json::to_value(&phase).expect("serialize phase");
        let back: Phase = serde_json::from_value(json).expect("re-parse phase");
        assert_eq!(back.exec, phase.exec);
    }

    #[test]
    fn phase_exec_ssh_parses_with_defaulted_nonzero() {
        let phase: Phase = serde_json::from_value(serde_json::json!({
            "id": "p3",
            "name": "Remote deploy",
            "objective": "push to the box",
            "exec": {
                "kind": "ssh",
                "host": "deploy@xbabe",
                "command": "bash ci-fast-push.sh",
            },
        }))
        .expect("ssh exec parses");
        match phase.exec {
            Some(PhaseExec::Ssh(ssh)) => {
                assert_eq!(ssh.host, "deploy@xbabe");
                assert_eq!(ssh.command, "bash ci-fast-push.sh");
                assert!(!ssh.allow_nonzero_exit, "defaults to failing on non-zero");
            }
            other => panic!("expected ssh exec, got {other:?}"),
        }
    }

    #[test]
    fn phase_exec_ssh_honors_allow_nonzero_exit() {
        let ssh = SshExec {
            host: "h".into(),
            command: "true".into(),
            allow_nonzero_exit: true,
        };
        let json = serde_json::to_value(PhaseExec::Ssh(ssh)).expect("serialize");
        assert_eq!(json["kind"], "ssh");
        assert_eq!(json["allow_nonzero_exit"], true);
    }

    #[test]
    fn phase_exec_jailgun_parses_and_round_trips() {
        let phase: Phase = serde_json::from_value(serde_json::json!({
            "id": "produce",
            "name": "Produce candidates",
            "objective": "same prompt across N tabs",
            "exec": {
                "kind": "jailgun",
                "prompt_ref": "jmcp://wo/42/prompt",
                "prompt": "implement the feature",
                "tabs": 5,
            },
        }))
        .expect("jailgun exec parses");
        match &phase.exec {
            Some(PhaseExec::Jailgun(jx)) => {
                assert_eq!(jx.prompt_ref, "jmcp://wo/42/prompt");
                assert_eq!(jx.prompt, "implement the feature");
                assert_eq!(jx.tabs, Some(5));
                assert_eq!(jx.max_runtime_seconds, None);
                assert!(
                    jx.request_overrides.is_null(),
                    "absent overrides default to null"
                );
            }
            other => panic!("expected jailgun exec, got {other:?}"),
        }
        let json = serde_json::to_value(&phase).expect("serialize");
        assert_eq!(json["exec"]["kind"], "jailgun");
        let back: Phase = serde_json::from_value(json).expect("re-parse");
        assert_eq!(back.exec, phase.exec);
    }
}
