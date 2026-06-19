//! Superreasoning runbook configuration.
//!
//! Holds the policy structs (`SuperReasoningConfig` and its sibling policies)
//! plus the shared constants. The macro-plan drafting/validation logic and
//! serde default-builder helpers live in the `plan` and `defaults`
//! submodules.

use serde::{Deserialize, Serialize};

use crate::model_client::CredentialSourcePolicy;

mod defaults;
mod plan;

pub use plan::{
    draft_super_master_plan, draft_super_master_plan_with_config, validate_super_macro_plan,
};

use defaults::{
    default_context_tokens, default_gap_task_prefix, default_gc_after,
    default_graph_slice_node_budget, default_graph_store, default_macro_stage_target,
    default_max_parallel_phases, default_max_workers, default_memory_write_requires,
    default_p95_ratio, default_per_phase_worker_cap, default_promotion_threshold,
    default_ramdisk_root, default_required_case_tags, default_retention_horizon, default_run_root,
    default_true, default_worktree_pool_size, default_worktree_root,
};

/// Superreasoning worker cap shared by live and deterministic workflows.
pub const MAX_SUPERREASONING_WORKERS: usize = 10;

/// Minimum macro-stage count for "full ambition" rewrite/port work.
pub const SUPER_STAGE_MIN: usize = 9;
/// Maximum macro-stage count before phase sprawl becomes harder than the work.
pub const SUPER_STAGE_MAX: usize = 12;
/// Default macro-stage target. Ten is large enough for full-stack parity work
/// while still forcing the reducer to merge overlapping ideas.
pub const DEFAULT_SUPER_STAGE_TARGET: usize = 10;

/// Runbook-level superreasoning options.
///
/// Combines the existing replay/parity/leak gate flags with the long-horizon
/// "super reasoning" policy knobs (parallel phases, active memory, graph
/// context, parity lab, persistent sandbox) needed for ambitious 9-12 stage
/// rewrite/port workloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuperReasoningConfig {
    /// Enable packet and gate artifacts.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Worker cap, clamped to [`MAX_SUPERREASONING_WORKERS`].
    #[serde(default = "default_max_workers")]
    pub max_workers: usize,
    /// Credential source policy for live child runs.
    #[serde(default)]
    pub credential_policy: CredentialSourcePolicy,
    /// Require negative memory artifacts.
    #[serde(default = "default_true")]
    pub require_negative_memory: bool,
    /// Require unsupported-claims ledger.
    #[serde(default = "default_true")]
    pub require_unsupported_claims_ledger: bool,
    /// Require replay receipt before completion.
    #[serde(default = "default_true")]
    pub require_replay_gate: bool,
    /// Require parity failures to block completion.
    #[serde(default = "default_true")]
    pub parity_fail_on_required: bool,
    /// Desired macro-stage count. Clamped to [`SUPER_STAGE_MIN`]..=[`SUPER_STAGE_MAX`].
    #[serde(default = "default_macro_stage_target")]
    pub macro_stage_target: usize,
    /// Parallel phase execution policy.
    #[serde(default)]
    pub parallel_phases: ParallelPhasePolicy,
    /// Active memory policy for knowledge compounding.
    #[serde(default)]
    pub active_memory: ActiveMemoryPolicy,
    /// Graph/context policy used to feed workers scoped code knowledge.
    #[serde(default)]
    pub graph: GraphContextPolicy,
    /// Target-switched parity and performance closure policy.
    #[serde(default)]
    pub parity: SuperParityPolicy,
    /// Persistent sandbox/worktree policy.
    #[serde(default)]
    pub sandbox: PersistentSandboxPolicy,
}

impl Default for SuperReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_workers: default_max_workers(),
            credential_policy: CredentialSourcePolicy::UsersOnly,
            require_negative_memory: true,
            require_unsupported_claims_ledger: true,
            require_replay_gate: true,
            parity_fail_on_required: true,
            macro_stage_target: default_macro_stage_target(),
            parallel_phases: ParallelPhasePolicy::default(),
            active_memory: ActiveMemoryPolicy::default(),
            graph: GraphContextPolicy::default(),
            parity: SuperParityPolicy::default(),
            sandbox: PersistentSandboxPolicy::default(),
        }
    }
}

impl SuperReasoningConfig {
    /// Return the effective worker cap.
    pub fn effective_max_workers(&self) -> usize {
        self.max_workers.clamp(1, MAX_SUPERREASONING_WORKERS)
    }

    /// Clamp stage target to the 9-12 macro-plane requested for ambitious
    /// rewrite/port projects.
    pub fn effective_stage_target(&self) -> usize {
        self.macro_stage_target
            .clamp(SUPER_STAGE_MIN, SUPER_STAGE_MAX)
    }
}

/// How phases may run concurrently within a super-reasoning macro plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelPhasePolicy {
    /// Enable phase-DAG scheduling where dependencies allow it.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum independent phases that may run at once. Capped by
    /// [`MAX_SUPERREASONING_WORKERS`] via [`Self::effective_max_parallel_phases`].
    #[serde(default = "default_max_parallel_phases")]
    pub max_parallel_phases: usize,
    /// Per-phase worker cap. Capped by [`MAX_SUPERREASONING_WORKERS`].
    #[serde(default = "default_per_phase_worker_cap")]
    pub per_phase_worker_cap: usize,
    /// Require explicit dependency edges before parallel execution.
    #[serde(default = "default_true")]
    pub require_dependency_edges: bool,
    /// Require workers in parallel phases to have disjoint write scopes.
    #[serde(default = "default_true")]
    pub disjoint_write_scopes_required: bool,
}

impl Default for ParallelPhasePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_parallel_phases: default_max_parallel_phases(),
            per_phase_worker_cap: default_per_phase_worker_cap(),
            require_dependency_edges: true,
            disjoint_write_scopes_required: true,
        }
    }
}

impl ParallelPhasePolicy {
    /// Effective parallel-phase count, clamped to the workspace worker cap.
    pub fn effective_max_parallel_phases(&self) -> usize {
        self.max_parallel_phases
            .clamp(1, MAX_SUPERREASONING_WORKERS)
    }

    /// Effective per-phase worker cap, clamped to the workspace worker cap.
    pub fn effective_per_phase_worker_cap(&self) -> usize {
        self.per_phase_worker_cap
            .clamp(1, MAX_SUPERREASONING_WORKERS)
    }
}

/// Memory is active only when it is structured, provenance-bound, and gated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveMemoryPolicy {
    /// Store run/event lessons.
    #[serde(default = "default_true")]
    pub episodic: bool,
    /// Store stable claims about the target/candidate behavior.
    #[serde(default = "default_true")]
    pub semantic: bool,
    /// Store reusable procedures only after verification.
    #[serde(default = "default_true")]
    pub procedural: bool,
    /// Store falsified approaches and failed hypotheses.
    #[serde(default = "default_true")]
    pub negative: bool,
    /// Evidence gates required before permanent memory writes.
    #[serde(default = "default_memory_write_requires")]
    pub write_requires: Vec<String>,
    /// Soft token cap for a worker memory/context pack.
    #[serde(default = "default_context_tokens")]
    pub max_context_tokens: usize,
    /// Promotion threshold (0.0..=1.0) for moving a tentative capsule to
    /// verified memory. Higher = more conservative.
    #[serde(default = "default_promotion_threshold")]
    pub promotion_threshold: f64,
    /// Soft retention horizon for tentative (not-yet-verified) capsules.
    #[serde(default = "default_retention_horizon")]
    pub retention_horizon: String,
}

impl Default for ActiveMemoryPolicy {
    fn default() -> Self {
        Self {
            episodic: true,
            semantic: true,
            procedural: true,
            negative: true,
            write_requires: default_memory_write_requires(),
            max_context_tokens: default_context_tokens(),
            promotion_threshold: default_promotion_threshold(),
            retention_horizon: default_retention_horizon(),
        }
    }
}

/// Repo-graph context policy.  The current implementation persists graph nodes
/// and edges in SQLite; this policy makes worker context generation explicit
/// and bounded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphContextPolicy {
    /// Backing store name. Kept as a string to allow sqlite, graphlite, or a
    /// future external graph DB without another schema bump.
    #[serde(default = "default_graph_store")]
    pub store: String,
    /// Update only touched slices when possible.
    #[serde(default = "default_true")]
    pub incremental: bool,
    /// Feed graph slices into worker prompts.
    #[serde(default = "default_true")]
    pub feed_workers: bool,
    /// Maximum graph nodes in a worker context pack.
    #[serde(default = "default_graph_slice_node_budget")]
    pub slice_node_budget: usize,
    /// Include tests connected to touched paths.
    #[serde(default = "default_true")]
    pub include_tests: bool,
    /// Include callers of touched functions/methods.
    #[serde(default = "default_true")]
    pub include_callers: bool,
    /// Include callees of touched functions/methods.
    #[serde(default = "default_true")]
    pub include_callees: bool,
}

impl Default for GraphContextPolicy {
    fn default() -> Self {
        Self {
            store: default_graph_store(),
            incremental: true,
            feed_workers: true,
            slice_node_budget: default_graph_slice_node_budget(),
            include_tests: true,
            include_callers: true,
            include_callees: true,
        }
    }
}

/// Parity/performance closure policy inspired by Redline-style evidence
/// bundles: generated manifests, approved case lists, raw JSONL, summaries,
/// gaps, and hash-bound reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuperParityPolicy {
    /// Tags required for cases that block completion.
    #[serde(default = "default_required_case_tags")]
    pub required_case_tags: Vec<String>,
    /// Prefix for spawned gap tasks.
    #[serde(default = "default_gap_task_prefix")]
    pub gap_task_prefix: String,
    /// Required cases must include performance data.
    #[serde(default = "default_true")]
    pub require_perf_data: bool,
    /// Prefer in-memory/tmpfs/RAM-disk execution for parity suites.
    #[serde(default = "default_true")]
    pub prefer_ramdisk: bool,
    /// Default ramdisk mount root for in-memory parity runs.
    #[serde(default = "default_ramdisk_root")]
    pub ramdisk_root: String,
    /// Default p95 candidate/reference budget.
    #[serde(default = "default_p95_ratio")]
    pub default_p95_ms_max_ratio: f64,
    /// Prefer in-memory exec (skip on-disk staging when safe).
    #[serde(default = "default_true")]
    pub in_memory_exec: bool,
}

impl Default for SuperParityPolicy {
    fn default() -> Self {
        Self {
            required_case_tags: default_required_case_tags(),
            gap_task_prefix: default_gap_task_prefix(),
            require_perf_data: true,
            prefer_ramdisk: true,
            ramdisk_root: default_ramdisk_root(),
            default_p95_ms_max_ratio: default_p95_ratio(),
            in_memory_exec: true,
        }
    }
}

/// Backend selection for the persistent sandbox / worktree pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxctlBackend {
    /// Native git worktrees.
    #[default]
    GitWorktree,
    /// Container-isolated worktrees (e.g. podman/docker run).
    Container,
    /// Local chroot/jail-style isolation.
    Chroot,
    /// Pure in-process (no isolation; testing only).
    InProcess,
}

/// Persistent sandbox policy for multi-hour or multi-day runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentSandboxPolicy {
    /// Backend selection for the sandbox/worktree pool.
    #[serde(default)]
    pub backend: SandboxctlBackend,
    /// Root for per-run state.
    #[serde(default = "default_run_root")]
    pub run_root: String,
    /// Root for worker worktrees.
    #[serde(default = "default_worktree_root")]
    pub worktree_root: String,
    /// Worktree pool maximum size (caps simultaneously checked-out worktrees).
    #[serde(default = "default_worktree_pool_size")]
    pub worktree_pool_size: usize,
    /// Preserve sandboxes between ticks so context and build caches compound.
    #[serde(default = "default_true")]
    pub keep_between_ticks: bool,
    /// Garbage collection horizon.
    #[serde(default = "default_gc_after")]
    pub gc_after: String,
    /// Reference repositories default to read-only.
    #[serde(default = "default_true")]
    pub read_only_reference_repos: bool,
}

impl Default for PersistentSandboxPolicy {
    fn default() -> Self {
        Self {
            backend: SandboxctlBackend::default(),
            run_root: default_run_root(),
            worktree_root: default_worktree_root(),
            worktree_pool_size: default_worktree_pool_size(),
            keep_between_ticks: true,
            gc_after: default_gc_after(),
            read_only_reference_repos: true,
        }
    }
}
