//! Policy types backing [`super::SuperReasoningPlan`].
//!
//! Split out of `super_reasoning/types.rs` to keep individual files under
//! the 500-LOC shape threshold (jankurai HLT-001:shape). All items are
//! re-exported by the parent module via `pub use policies::*;`, so external
//! callers continue to import from `crate::daemon::super_reasoning::Foo`.

use serde::{Deserialize, Serialize};

/// Swarm/fleet behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwarmPolicy {
    /// Maximum concurrently active workers.
    pub max_workers: u16,
    /// Worker isolation strategy.
    pub isolation: IsolationMode,
    /// Whether independent phases may run in parallel waves.
    pub parallel_phase_mode: bool,
    /// Number of redundant weak-agent attempts for important tasks.
    pub weak_agent_redundancy: u16,
    /// Percent of workers reserved for critics/reviewers.
    pub critic_ratio_percent: u8,
    /// Reducer quorum required for promotion.
    pub reducer_quorum: u16,
    /// Worktree branch prefix.
    pub worktree_branch_prefix: String,
    /// Integration branch name.
    pub integration_branch: String,
}

/// Worker isolation strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    /// Same session, no separate worktree.
    SameSession,
    /// Git worktree per worker/phase.
    GitWorktree,
    /// Persistent sandbox with retained caches/artifacts.
    PersistentSandbox,
    /// Ephemeral sandbox per task.
    EphemeralSandbox,
}

/// Memory compounding policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCompoundingPolicy {
    /// Enable active memory.
    pub active_memory: bool,
    /// Store declarations.
    pub stores: Vec<MemoryStorePlan>,
    /// Promotion rules between stores.
    pub promotion_rules: Vec<MemoryPromotionRule>,
    /// Compression cadence.
    pub compression_every_iterations: u16,
    /// Preserve negative results and failed hypotheses.
    pub preserve_negative_results: bool,
    /// Reasoning trace persistence policy.
    pub reasoning_trace_policy: ReasoningTracePolicy,
}

impl Default for MemoryCompoundingPolicy {
    fn default() -> Self {
        Self {
            active_memory: true,
            stores: vec![
                MemoryStorePlan {
                    id: "phase_receipts".to_string(),
                    scope: "phase".to_string(),
                    retention: "mission".to_string(),
                    write_policy: "append_only".to_string(),
                    searchable: true,
                    path: Some(".jekko/memory/phase-receipts".to_string()),
                },
                MemoryStorePlan {
                    id: "parity_gaps".to_string(),
                    scope: "mission".to_string(),
                    retention: "project".to_string(),
                    write_policy: "upsert".to_string(),
                    searchable: true,
                    path: Some(".jekko/memory/parity-gaps".to_string()),
                },
                MemoryStorePlan {
                    id: "concept_memory".to_string(),
                    scope: "project".to_string(),
                    retention: "permanent".to_string(),
                    write_policy: "verified_upsert".to_string(),
                    searchable: true,
                    path: Some(".jekko/memory/concepts".to_string()),
                },
            ],
            promotion_rules: vec![MemoryPromotionRule {
                from: "phase_receipts".to_string(),
                to: "concept_memory".to_string(),
                condition: "verified_reused_or_phase_signed_off".to_string(),
                evidence_required: vec!["phase_evidence_bundle".to_string()],
            }],
            compression_every_iterations: 4,
            preserve_negative_results: true,
            reasoning_trace_policy: ReasoningTracePolicy::ReceiptsOnly,
        }
    }
}

/// Memory store declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStorePlan {
    /// Store id.
    pub id: String,
    /// Store scope.
    pub scope: String,
    /// Retention policy.
    pub retention: String,
    /// Write policy.
    pub write_policy: String,
    /// Whether this store is searchable.
    pub searchable: bool,
    /// Optional filesystem path.
    pub path: Option<String>,
}

/// Memory promotion rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPromotionRule {
    /// Source store.
    pub from: String,
    /// Destination store.
    pub to: String,
    /// Promotion condition.
    pub condition: String,
    /// Evidence required for promotion.
    #[serde(default)]
    pub evidence_required: Vec<String>,
}

/// Policy for persistent reasoning traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningTracePolicy {
    /// Store only receipts, claims, evidence pointers, and decisions.
    ReceiptsOnly,
    /// Store short summaries only.
    SummariesOnly,
    /// Store no reasoning traces.
    Disabled,
}

/// Repository graph/indexing policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoGraphPolicy {
    /// Enable repository graph support.
    pub enabled: bool,
    /// Backing graph store.
    pub store: GraphStore,
    /// Index functions/symbols.
    pub index_functions: bool,
    /// Index tests and test-to-code edges.
    pub index_tests: bool,
    /// Index call/import edges.
    pub index_call_edges: bool,
    /// Index dataflow hints where available.
    pub index_dataflow: bool,
    /// Refresh policy.
    pub refresh: Vec<IndexRefreshPolicy>,
    /// Expected graph artifacts.
    pub artifacts: Vec<String>,
}

impl Default for RepoGraphPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            store: GraphStore::Sqlite,
            index_functions: true,
            index_tests: true,
            index_call_edges: true,
            index_dataflow: true,
            refresh: vec![
                IndexRefreshPolicy::OnStart,
                IndexRefreshPolicy::OnPhaseEntry,
                IndexRefreshPolicy::OnCheckpoint,
            ],
            artifacts: vec![
                ".jekko/repo-graph/graph.sqlite".to_string(),
                ".jekko/repo-graph/atlas.json".to_string(),
            ],
        }
    }
}

/// Graph store implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoGraphStore {
    /// SQLite adjacency tables.
    Sqlite,
    /// Kuzu graph database.
    Kuzu,
    /// Neo4j graph database.
    Neo4j,
    /// Tantivy/vector hybrid index with explicit edges.
    TantivyHybrid,
}

pub use RepoGraphStore as GraphStore;

/// When to refresh the repo graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexRefreshPolicy {
    /// Refresh at mission start.
    OnStart,
    /// Refresh at every phase entry.
    OnPhaseEntry,
    /// Refresh after verified checkpoint.
    OnCheckpoint,
    /// Refresh when git changes are detected.
    OnGitChange,
}

/// Persistent sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentSandboxPolicy {
    /// Enable persistent sandbox support.
    pub enabled: bool,
    /// Sandbox root.
    pub root: String,
    /// Keep sandbox between phases.
    pub keep_between_phases: bool,
    /// Optional ramdisk policy for fast parity/perf tests.
    pub ramdisk: Option<RamDiskPolicy>,
    /// Environment keys denied from sandbox processes.
    pub env_deny: Vec<String>,
    /// Network policy.
    pub network: String,
}

impl Default for PersistentSandboxPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            root: ".jekko/sandboxes/${mission_id}".to_string(),
            keep_between_phases: true,
            ramdisk: Some(RamDiskPolicy {
                enabled: true,
                path: "/dev/shm/jekko-${mission_id}".to_string(),
                size: "8G".to_string(),
            }),
            env_deny: vec!["*_TOKEN".to_string(), "*_SECRET".to_string()],
            network: "deny_during_implementation_allowlist_during_research".to_string(),
        }
    }
}

/// Ramdisk policy for high-throughput local test loops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RamDiskPolicy {
    /// Enable ramdisk use.
    pub enabled: bool,
    /// Ramdisk path.
    pub path: String,
    /// Requested size.
    pub size: String,
}

/// Generic parity closure policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParityClosurePolicy {
    /// Enable parity closure.
    pub enabled: bool,
    /// Parity workflow types.
    pub workflows: Vec<ParityWorkflow>,
    /// Command template for the reference implementation.
    pub reference_command: String,
    /// Command template for the candidate implementation.
    pub candidate_command: String,
    /// Path to the parity manifest.
    pub manifest: String,
    /// Path to or command for the parity oracle.
    pub oracle: String,
    /// Close gaps until this condition is satisfied.
    pub close_gaps_until: String,
    /// Optional performance budget command.
    pub performance_budget_command: Option<String>,
}

impl Default for ParityClosurePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            workflows: vec![
                ParityWorkflow::Differential,
                ParityWorkflow::GoldenCorpus,
                ParityWorkflow::FuzzDifferential,
                ParityWorkflow::Metamorphic,
            ],
            reference_command: "./scripts/parity-reference.sh ${case}".to_string(),
            candidate_command: "./scripts/parity-candidate.sh ${case}".to_string(),
            manifest: "tests/parity/manifest.jsonl".to_string(),
            oracle: "./scripts/parity-oracle.sh".to_string(),
            close_gaps_until: "blocking_gaps_zero".to_string(),
            performance_budget_command: Some("./scripts/perf-budget.sh".to_string()),
        }
    }
}

/// Generic parity workflow types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParityWorkflow {
    /// Reference-vs-candidate differential tests.
    Differential,
    /// Golden corpus replay.
    GoldenCorpus,
    /// Metamorphic behavior checks.
    Metamorphic,
    /// Fuzz-generated differential tests.
    FuzzDifferential,
    /// Property-based differential tests.
    PropertyDifferential,
}

/// Hardening policy after parity is established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardeningPolicy {
    /// Require coverage evidence.
    pub require_test_coverage: bool,
    /// Require fuzzing/stress evidence.
    pub require_fuzzing: bool,
    /// Require performance baseline evidence.
    pub require_perf_baseline: bool,
    /// Require security review evidence.
    pub require_security_review: bool,
    /// Retry gap closure until gates pass.
    pub retry_gaps_until_green: bool,
}

impl Default for HardeningPolicy {
    fn default() -> Self {
        Self {
            require_test_coverage: true,
            require_fuzzing: true,
            require_perf_baseline: true,
            require_security_review: true,
            retry_gaps_until_green: true,
        }
    }
}

/// Final mission signoff policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignoffPolicy {
    /// Require every phase to complete.
    pub require_all_phases: bool,
    /// Require evidence bundle.
    pub require_evidence_bundle: bool,
    /// Require parity closure if parity is enabled.
    pub require_parity_closure: bool,
    /// Require performance closure if performance budget is configured.
    pub require_performance_closure: bool,
    /// Require human review.
    pub require_human_review: bool,
    /// Final receipt output path.
    pub final_receipt_path: String,
}

impl Default for SignoffPolicy {
    fn default() -> Self {
        Self {
            require_all_phases: true,
            require_evidence_bundle: true,
            require_parity_closure: true,
            require_performance_closure: true,
            require_human_review: true,
            final_receipt_path: "target/zyal/super-reasoning/final-receipt.json".to_string(),
        }
    }
}
