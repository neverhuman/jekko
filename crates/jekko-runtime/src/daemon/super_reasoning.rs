//! Typed control-plane support for very large ZYAL reasoning missions.
//!
//! The existing ZYAL contract already has the required authoring primitives
//! (`workflow`, `fleet`, `memory`, `evidence`, `repo_intelligence`,
//! `approvals`, `sandbox`, `checkpoint`, `done`). This module provides the
//! runtime-normalised shape that binds those blocks into a durable phase DAG
//! for multi-agent, multi-day work without adding target-specific schema such
//! as "Redis" or "SQLite" to the contract.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::{RuntimeError, RuntimeResult};

/// Metadata key used on [`crate::daemon::DaemonRecord::metadata`].
pub const SUPER_REASONING_METADATA_KEY: &str = "super_reasoning";

/// Schema version for the runtime-normalised plan payload.
pub const SUPER_REASONING_SCHEMA_VERSION: &str = "super_reasoning/v1";

/// Default maximum worker cap aligned with the current flagship fleet examples.
pub const DEFAULT_MAX_WORKERS: u16 = 20;

/// Default lower bound for macro-phase plans.
pub const DEFAULT_MIN_PHASES: usize = 9;

/// Default upper bound for macro-phase plans.
pub const DEFAULT_MAX_PHASES: usize = 12;

/// A host-normalised mission plan for ambitious ZYAL workflows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuperReasoningPlan {
    /// Plan schema version.
    pub schema_version: String,
    /// Stable mission id.
    pub mission_id: String,
    /// Human objective for the whole mission.
    pub objective: String,
    /// Required macro-phase envelope.
    pub phase_count: PhaseCountTarget,
    /// Swarm/fleet behavior.
    pub swarm: SwarmPolicy,
    /// Memory compounding policy.
    pub memory: MemoryCompoundingPolicy,
    /// Repository graph/indexing policy.
    pub repo_graph: RepoGraphPolicy,
    /// Persistent sandbox policy.
    pub sandbox: PersistentSandboxPolicy,
    /// Generic parity closure policy.
    pub parity: Option<ParityClosurePolicy>,
    /// Hardening and safety proof policy.
    pub hardening: HardeningPolicy,
    /// Final mission signoff policy.
    pub signoff: SignoffPolicy,
    /// Macro phases that make up the mission.
    pub phases: Vec<MacroPhase>,
}

impl SuperReasoningPlan {
    /// Create the canonical 12-stage mega-project plan.
    ///
    /// This intentionally avoids target-specific assumptions. It can represent
    /// a database rewrite, compiler port, protocol-compatible replacement,
    /// research program, or large refactor as long as parity and performance
    /// policies are supplied by the host/runbook.
    pub fn default_megaproject_plan(
        mission_id: impl Into<String>,
        objective: impl Into<String>,
    ) -> Self {
        let mission_id = mission_id.into();
        let objective = objective.into();
        Self {
            schema_version: SUPER_REASONING_SCHEMA_VERSION.to_string(),
            mission_id,
            objective,
            phase_count: PhaseCountTarget {
                min: DEFAULT_MIN_PHASES,
                max: DEFAULT_MAX_PHASES,
            },
            swarm: SwarmPolicy {
                max_workers: DEFAULT_MAX_WORKERS,
                isolation: IsolationMode::GitWorktree,
                parallel_phase_mode: true,
                weak_agent_redundancy: 3,
                critic_ratio_percent: 25,
                reducer_quorum: 2,
                worktree_branch_prefix: "zyal/super".to_string(),
                integration_branch: "zyal_super_integration".to_string(),
            },
            memory: MemoryCompoundingPolicy::default(),
            repo_graph: RepoGraphPolicy::default(),
            sandbox: PersistentSandboxPolicy::default(),
            parity: Some(ParityClosurePolicy::default()),
            hardening: HardeningPolicy::default(),
            signoff: SignoffPolicy::default(),
            phases: canonical_phases(),
        }
    }

    /// Validate the plan before daemon registration or execution.
    pub fn validate(&self) -> RuntimeResult<()> {
        if self.schema_version != SUPER_REASONING_SCHEMA_VERSION {
            return Err(RuntimeError::invalid(format!(
                "unsupported super reasoning schema '{}', expected '{}'",
                self.schema_version, SUPER_REASONING_SCHEMA_VERSION
            )));
        }
        if self.mission_id.trim().is_empty() {
            return Err(RuntimeError::invalid(
                "super reasoning mission_id is required",
            ));
        }
        if self.objective.trim().is_empty() {
            return Err(RuntimeError::invalid(
                "super reasoning objective is required",
            ));
        }
        if self.phase_count.min == 0 || self.phase_count.min > self.phase_count.max {
            return Err(RuntimeError::invalid(
                "phase_count must have min > 0 and min <= max",
            ));
        }
        let phase_len = self.phases.len();
        if phase_len < self.phase_count.min || phase_len > self.phase_count.max {
            return Err(RuntimeError::invalid(format!(
                "super reasoning plans require {}-{} phases; got {}",
                self.phase_count.min, self.phase_count.max, phase_len
            )));
        }
        if self.swarm.max_workers == 0 || self.swarm.max_workers > DEFAULT_MAX_WORKERS {
            return Err(RuntimeError::invalid(format!(
                "swarm.max_workers must be between 1 and {DEFAULT_MAX_WORKERS}"
            )));
        }
        if self.swarm.weak_agent_redundancy == 0 {
            return Err(RuntimeError::invalid(
                "swarm.weak_agent_redundancy must be at least 1",
            ));
        }
        if self.swarm.critic_ratio_percent > 100 {
            return Err(RuntimeError::invalid(
                "swarm.critic_ratio_percent cannot exceed 100",
            ));
        }

        let mut ids = BTreeSet::new();
        for phase in &self.phases {
            if phase.id.trim().is_empty() {
                return Err(RuntimeError::invalid("phase id is required"));
            }
            if !ids.insert(phase.id.clone()) {
                return Err(RuntimeError::invalid(format!(
                    "duplicate phase id '{}'",
                    phase.id
                )));
            }
            if phase.name.trim().is_empty() || phase.objective.trim().is_empty() {
                return Err(RuntimeError::invalid(format!(
                    "phase '{}' requires name and objective",
                    phase.id
                )));
            }
            if phase.workers == 0 || phase.workers > self.swarm.max_workers {
                return Err(RuntimeError::invalid(format!(
                    "phase '{}' workers must be between 1 and swarm.max_workers ({})",
                    phase.id, self.swarm.max_workers
                )));
            }
            if phase.tasks.is_empty() {
                return Err(RuntimeError::invalid(format!(
                    "phase '{}' must declare at least one task",
                    phase.id
                )));
            }
            if phase.lanes.is_empty() {
                return Err(RuntimeError::invalid(format!(
                    "phase '{}' must declare at least one reasoning lane",
                    phase.id
                )));
            }
            if phase.acceptance.is_empty() {
                return Err(RuntimeError::invalid(format!(
                    "phase '{}' must declare at least one acceptance gate",
                    phase.id
                )));
            }
            for dep in &phase.depends_on {
                if dep == &phase.id {
                    return Err(RuntimeError::invalid(format!(
                        "phase '{}' cannot depend on itself",
                        phase.id
                    )));
                }
            }
        }

        let _ = self.topological_phase_ids()?;

        if let Some(parity) = &self.parity {
            if parity.enabled {
                if parity.workflows.is_empty() {
                    return Err(RuntimeError::invalid(
                        "enabled parity policy requires at least one workflow",
                    ));
                }
                if parity.reference_command.trim().is_empty()
                    || parity.candidate_command.trim().is_empty()
                    || parity.manifest.trim().is_empty()
                    || parity.oracle.trim().is_empty()
                {
                    return Err(RuntimeError::invalid(
                        "enabled parity policy requires reference_command, candidate_command, manifest, and oracle",
                    ));
                }
            }
        }

        Ok(())
    }

    /// Return phase ids in deterministic topological order.
    pub fn topological_phase_ids(&self) -> RuntimeResult<Vec<String>> {
        let (mut indegree, mut children) = self.dependency_maps()?;
        let mut queue: VecDeque<String> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then(|| id.clone()))
            .collect();
        let mut out = Vec::with_capacity(self.phases.len());
        while let Some(id) = queue.pop_front() {
            out.push(id.clone());
            if let Some(next) = children.remove(&id) {
                for child in next {
                    let degree = indegree
                        .get_mut(&child)
                        .ok_or_else(|| RuntimeError::invalid("dependency map corrupt"))?;
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
        if out.len() != self.phases.len() {
            return Err(RuntimeError::invalid(
                "super reasoning phase dependency graph contains a cycle",
            ));
        }
        Ok(out)
    }

    /// Return deterministic parallel waves from the phase DAG.
    pub fn parallel_waves(&self) -> RuntimeResult<Vec<Vec<String>>> {
        let mut remaining: BTreeMap<String, BTreeSet<String>> = self
            .phases
            .iter()
            .map(|phase| {
                (
                    phase.id.clone(),
                    phase.depends_on.iter().cloned().collect::<BTreeSet<_>>(),
                )
            })
            .collect();
        let valid_ids: BTreeSet<String> = remaining.keys().cloned().collect();
        for (id, deps) in &remaining {
            for dep in deps {
                if !valid_ids.contains(dep) {
                    return Err(RuntimeError::invalid(format!(
                        "phase '{id}' depends on unknown phase '{dep}'"
                    )));
                }
            }
        }

        let mut waves = Vec::new();
        while !remaining.is_empty() {
            let wave: Vec<String> = remaining
                .iter()
                .filter_map(|(id, deps)| deps.is_empty().then(|| id.clone()))
                .collect();
            if wave.is_empty() {
                return Err(RuntimeError::invalid(
                    "super reasoning phase dependency graph contains a cycle",
                ));
            }
            for id in &wave {
                remaining.remove(id);
            }
            for deps in remaining.values_mut() {
                for id in &wave {
                    deps.remove(id);
                }
            }
            waves.push(wave);
        }
        Ok(waves)
    }

    /// Return phases that are runnable given a set of completed phase ids.
    pub fn ready_phase_ids<I, S>(&self, completed_phase_ids: I) -> RuntimeResult<Vec<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let completed: BTreeSet<String> = completed_phase_ids
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let valid_ids: BTreeSet<String> = self.phases.iter().map(|p| p.id.clone()).collect();
        for id in &completed {
            if !valid_ids.contains(id) {
                return Err(RuntimeError::invalid(format!(
                    "completed phase '{id}' is not in this plan"
                )));
            }
        }
        Ok(self
            .phases
            .iter()
            .filter(|phase| {
                !completed.contains(&phase.id)
                    && phase.depends_on.iter().all(|dep| completed.contains(dep))
            })
            .map(|phase| phase.id.clone())
            .collect())
    }

    /// Return the ZYAL blocks that should be present in the source runbook.
    pub fn required_zyal_blocks(&self) -> Vec<&'static str> {
        let mut blocks = vec![
            "workflow",
            "fleet",
            "memory",
            "evidence",
            "approvals",
            "budgets",
            "sandbox",
            "observability",
            "checkpoint",
            "done",
        ];
        if self.repo_graph.enabled {
            blocks.push("repo_intelligence");
        }
        if self.parity.as_ref().is_some_and(|p| p.enabled) {
            blocks.push("hooks");
        }
        blocks
    }

    fn dependency_maps(
        &self,
    ) -> RuntimeResult<(BTreeMap<String, usize>, BTreeMap<String, Vec<String>>)> {
        let mut ids = BTreeSet::new();
        for phase in &self.phases {
            if !ids.insert(phase.id.clone()) {
                return Err(RuntimeError::invalid(format!(
                    "duplicate phase id '{}'",
                    phase.id
                )));
            }
        }
        let mut indegree: BTreeMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for phase in &self.phases {
            // Dedupe deps per phase — a duplicate entry (e.g. `depends_on:
            // ["p01", "p01"]`) would inflate indegree past what the topo
            // walk can decrement, falsely reporting a cycle.
            let unique_deps: std::collections::BTreeSet<&String> =
                phase.depends_on.iter().collect();
            for dep in unique_deps {
                if !ids.contains(dep) {
                    return Err(RuntimeError::invalid(format!(
                        "phase '{}' depends on unknown phase '{}'",
                        phase.id, dep
                    )));
                }
                *indegree
                    .get_mut(&phase.id)
                    .ok_or_else(|| RuntimeError::invalid("dependency map corrupt"))? += 1;
                children
                    .entry(dep.clone())
                    .or_default()
                    .push(phase.id.clone());
            }
        }
        for child_list in children.values_mut() {
            child_list.sort();
        }
        Ok((indegree, children))
    }
}

/// Required macro-phase envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseCountTarget {
    /// Minimum accepted phase count.
    pub min: usize,
    /// Maximum accepted phase count.
    pub max: usize,
}

/// A major phase in a long-running reasoning mission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroPhase {
    /// Stable phase id.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Phase objective.
    pub objective: String,
    /// Phase ids that must complete first.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Advisory parallel peers.
    #[serde(default)]
    pub can_run_parallel_with: Vec<String>,
    /// Bounded tasks under the phase.
    pub tasks: Vec<ReasoningTask>,
    /// Worker cap for the phase.
    pub workers: u16,
    /// Reasoning lanes used inside the phase.
    pub lanes: Vec<ReasoningLane>,
    /// Acceptance gates that block phase promotion.
    pub acceptance: Vec<AcceptanceGate>,
    /// Phase-level budget.
    pub budget: PhaseBudget,
    /// Phase signoff mode.
    pub signoff: PhaseSignoffMode,
    /// Strategy used to combine worker output.
    pub fuse_strategy: FuseStrategy,
    /// Maximum write scope for this phase.
    pub writes: WriteScope,
    /// Expected artifacts.
    #[serde(default)]
    pub artifacts: Vec<String>,
}

/// A bounded task that can be assigned to workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningTask {
    /// Stable task id.
    pub id: String,
    /// Task description.
    pub description: String,
    /// Preferred role or skill owner.
    pub owner_role: String,
    /// Expected artifacts from the task.
    #[serde(default)]
    pub produces: Vec<String>,
    /// Task ids or artifact ids that block this task.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Whether a failing test or executable proof should precede implementation.
    pub test_first: bool,
}

/// A reasoning lane type used by the phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningLane {
    /// Stable lane id.
    pub id: String,
    /// Lane role, such as scout, implementer, critic, reducer, or reviewer.
    pub role: String,
    /// Number of workers or attempts for this lane.
    pub count: u16,
    /// Context policy for the lane.
    pub context_policy: String,
    /// Whether this lane is intentionally blind to other lanes.
    pub blind: bool,
    /// Lane write scope.
    pub writes: WriteScope,
}

/// Write scope enforced for a phase or lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteScope {
    /// No writes; read-only analysis.
    ReadOnly,
    /// Writes only to scratch/receipts.
    ScratchOnly,
    /// Writes in an isolated worktree.
    IsolatedWorktree,
    /// Writes in an integration branch/worktree.
    IntegrationBranch,
    /// Writes in the main worktree.
    MainWorktree,
}

/// Gate that must be satisfied for promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceGate {
    /// Gate kind.
    pub kind: AcceptanceGateKind,
    /// Optional shell command that proves the gate.
    pub command: Option<String>,
    /// Artifacts that must exist before the gate passes.
    #[serde(default)]
    pub required_artifacts: Vec<String>,
    /// Minimum reviewer quorum for reviewer gates.
    pub min_reviewer_quorum: Option<u16>,
    /// Whether this gate blocks promotion.
    pub blocks_promotion: bool,
}

/// Acceptance gate kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceGateKind {
    /// Plan receipt exists.
    PlanReceipt,
    /// Evidence bundle exists and is signed.
    EvidenceBundle,
    /// Tests are green.
    TestsGreen,
    /// Parity suite threshold passes.
    ParitySuite,
    /// Performance budget passes.
    PerformanceBudget,
    /// Security or hardening review passes.
    SecurityReview,
    /// Repository graph is fresh enough.
    RepoGraphFresh,
    /// Human approval is granted.
    HumanApproval,
    /// Reviewer quorum is reached.
    ReviewerQuorum,
    /// No critical objections remain open.
    NoOpenCriticalObjections,
}

/// Phase-level budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseBudget {
    /// Maximum iterations before park/incubate/escalate.
    pub max_iterations: u16,
    /// Maximum wall-clock time string.
    pub max_wall_clock: String,
    /// Maximum diff lines for the phase.
    pub max_diff_lines: u32,
    /// Optional cost budget as a string to avoid currency/float assumptions.
    pub max_cost: Option<String>,
}

/// Signoff style for a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseSignoffMode {
    /// Gate-only signoff.
    Automatic,
    /// Reviewer agent signoff.
    Reviewer,
    /// Human signoff.
    Human,
    /// Evidence-only signoff.
    EvidenceOnly,
}

/// Strategy used to combine worker outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuseStrategy {
    /// Keep the best verified lane result.
    BestVerified,
    /// Synthesize the strongest verified fragments.
    SynthesizeBest,
    /// Merge non-conflicting verified worktree outputs.
    MergeNonConflicting,
    /// Integrate sequentially with a proof after each merge.
    SequentialIntegration,
    /// Block on reviewer adjudication.
    BlockingReview,
}

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
pub enum GraphStore {
    /// SQLite adjacency tables.
    Sqlite,
    /// Kuzu graph database.
    Kuzu,
    /// Neo4j graph database.
    Neo4j,
    /// Tantivy/vector hybrid index with explicit edges.
    TantivyHybrid,
}

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

fn canonical_phases() -> Vec<MacroPhase> {
    vec![
        phase(
            "source_of_truth",
            "Source of truth",
            "Build the non-negotiable behavior, API, compatibility, and evidence ledger.",
            vec![],
            WriteScope::ScratchOnly,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SynthesizeBest,
            AcceptanceGateKind::EvidenceBundle,
        ),
        phase(
            "architecture_blueprint",
            "Architecture blueprint",
            "Convert requirements into module boundaries, invariants, risk register, and implementation slices.",
            vec!["source_of_truth"],
            WriteScope::ScratchOnly,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SynthesizeBest,
            AcceptanceGateKind::PlanReceipt,
        ),
        phase(
            "repo_graph_bootstrap",
            "Repo graph bootstrap",
            "Index functions, symbols, tests, call edges, dataflow hints, ownership, and blast radius.",
            vec!["source_of_truth"],
            WriteScope::ScratchOnly,
            PhaseSignoffMode::EvidenceOnly,
            FuseStrategy::BestVerified,
            AcceptanceGateKind::RepoGraphFresh,
        ),
        phase(
            "contracts_and_slices",
            "Contracts and slices",
            "Produce task contracts, proof commands, fixture strategy, and independent slices.",
            vec!["architecture_blueprint", "repo_graph_bootstrap"],
            WriteScope::ScratchOnly,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SynthesizeBest,
            AcceptanceGateKind::PlanReceipt,
        ),
        phase(
            "parallel_subsystems",
            "Parallel subsystems",
            "Implement independent verified slices in isolated worktrees with critic lanes.",
            vec!["contracts_and_slices"],
            WriteScope::IsolatedWorktree,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::MergeNonConflicting,
            AcceptanceGateKind::TestsGreen,
        ),
        phase(
            "integration_fusion",
            "Integration fusion",
            "Fuse verified subsystem work, resolve interface drift, and run integration proof lanes.",
            vec!["parallel_subsystems"],
            WriteScope::IntegrationBranch,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SequentialIntegration,
            AcceptanceGateKind::TestsGreen,
        ),
        phase(
            "parity_lab",
            "Parity lab",
            "Create differential, golden, metamorphic, and fuzz parity harnesses.",
            vec!["source_of_truth", "contracts_and_slices"],
            WriteScope::IsolatedWorktree,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SynthesizeBest,
            AcceptanceGateKind::ParitySuite,
        ),
        phase(
            "parity_gap_closure",
            "Parity gap closure",
            "Close blocking parity gaps using the gap ledger and regression proofs.",
            vec!["integration_fusion", "parity_lab"],
            WriteScope::IntegrationBranch,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SequentialIntegration,
            AcceptanceGateKind::ParitySuite,
        ),
        phase(
            "performance_closure",
            "Performance closure",
            "Benchmark reference versus candidate and close hot-path gaps without weakening parity.",
            vec!["parity_gap_closure"],
            WriteScope::IntegrationBranch,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::SequentialIntegration,
            AcceptanceGateKind::PerformanceBudget,
        ),
        phase(
            "hardening_security",
            "Hardening and security",
            "Run fuzzing, stress, recovery, race, security, and fault-injection proof lanes.",
            vec!["parity_gap_closure"],
            WriteScope::IntegrationBranch,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::BestVerified,
            AcceptanceGateKind::SecurityReview,
        ),
        phase(
            "docs_release_ops",
            "Docs, release, and operations",
            "Produce docs, CI gates, migration notes, release checklist, and operational runbooks.",
            vec!["performance_closure", "hardening_security"],
            WriteScope::IsolatedWorktree,
            PhaseSignoffMode::Reviewer,
            FuseStrategy::MergeNonConflicting,
            AcceptanceGateKind::EvidenceBundle,
        ),
        phase(
            "final_signoff",
            "Final signoff",
            "Aggregate receipts, rerun full proofs, ensure clean tree, and require final approval.",
            vec!["docs_release_ops"],
            WriteScope::ReadOnly,
            PhaseSignoffMode::Human,
            FuseStrategy::BlockingReview,
            AcceptanceGateKind::HumanApproval,
        ),
    ]
}

fn phase(
    id: &str,
    name: &str,
    objective: &str,
    depends_on: Vec<&str>,
    writes: WriteScope,
    signoff: PhaseSignoffMode,
    fuse_strategy: FuseStrategy,
    primary_gate: AcceptanceGateKind,
) -> MacroPhase {
    MacroPhase {
        id: id.to_string(),
        name: name.to_string(),
        objective: objective.to_string(),
        depends_on: depends_on.into_iter().map(str::to_string).collect(),
        can_run_parallel_with: Vec::new(),
        tasks: vec![ReasoningTask {
            id: format!("{id}.plan"),
            description: format!("Produce and verify the {name} phase receipt."),
            owner_role: "phase_lead".to_string(),
            produces: vec![format!("target/zyal/super-reasoning/{id}/receipt.json")],
            blocked_by: Vec::new(),
            test_first: matches!(
                primary_gate,
                AcceptanceGateKind::TestsGreen
                    | AcceptanceGateKind::ParitySuite
                    | AcceptanceGateKind::PerformanceBudget
            ),
        }],
        workers: match writes {
            WriteScope::ReadOnly | WriteScope::ScratchOnly => 4,
            WriteScope::IsolatedWorktree => 8,
            WriteScope::IntegrationBranch => 6,
            WriteScope::MainWorktree => 1,
        },
        lanes: default_lanes(writes),
        acceptance: vec![
            AcceptanceGate {
                kind: primary_gate,
                command: None,
                required_artifacts: vec![format!("target/zyal/super-reasoning/{id}/receipt.json")],
                min_reviewer_quorum: Some(2),
                blocks_promotion: true,
            },
            AcceptanceGate {
                kind: AcceptanceGateKind::NoOpenCriticalObjections,
                command: None,
                required_artifacts: Vec::new(),
                min_reviewer_quorum: Some(1),
                blocks_promotion: true,
            },
        ],
        budget: PhaseBudget {
            max_iterations: 12,
            max_wall_clock: "24h".to_string(),
            max_diff_lines: 5_000,
            max_cost: None,
        },
        signoff,
        fuse_strategy,
        writes,
        artifacts: vec![format!("target/zyal/super-reasoning/{id}/receipt.json")],
    }
}

fn default_lanes(writes: WriteScope) -> Vec<ReasoningLane> {
    vec![
        ReasoningLane {
            id: "blind_scout".to_string(),
            role: "scout".to_string(),
            count: 2,
            context_policy: "blind_minimal".to_string(),
            blind: true,
            writes: WriteScope::ScratchOnly,
        },
        ReasoningLane {
            id: "builder".to_string(),
            role: "implementer".to_string(),
            count: 3,
            context_policy: "graph_and_phase_memory".to_string(),
            blind: false,
            writes,
        },
        ReasoningLane {
            id: "critic".to_string(),
            role: "critic".to_string(),
            count: 2,
            context_policy: "evidence_and_diff_only".to_string(),
            blind: false,
            writes: WriteScope::ScratchOnly,
        },
        ReasoningLane {
            id: "reducer".to_string(),
            role: "reducer".to_string(),
            count: 1,
            context_policy: "candidate_pool".to_string(),
            blind: false,
            writes: WriteScope::ScratchOnly,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plan_validates() {
        let plan = SuperReasoningPlan::default_megaproject_plan(
            "mission_1",
            "Build a reference-compatible replacement and close parity/perf gaps.",
        );
        plan.validate().unwrap();
        assert_eq!(plan.phases.len(), 12);
        assert_eq!(
            plan.topological_phase_ids().unwrap().first().unwrap(),
            "source_of_truth"
        );
    }

    #[test]
    fn ready_phases_respect_dependencies() {
        let plan = SuperReasoningPlan::default_megaproject_plan("mission_1", "objective");
        assert_eq!(
            plan.ready_phase_ids(std::iter::empty::<String>()).unwrap(),
            vec!["source_of_truth"]
        );

        let ready = plan
            .ready_phase_ids(["source_of_truth".to_string()])
            .unwrap();
        assert!(ready.contains(&"architecture_blueprint".to_string()));
        assert!(ready.contains(&"repo_graph_bootstrap".to_string()));
    }

    #[test]
    fn rejects_cycles() {
        let mut plan = SuperReasoningPlan::default_megaproject_plan("mission_1", "objective");
        plan.phases[0]
            .depends_on
            .push("architecture_blueprint".to_string());
        assert!(plan.validate().is_err());
    }

    #[test]
    fn rejects_over_cap_worker_plan() {
        let mut plan = SuperReasoningPlan::default_megaproject_plan("mission_1", "objective");
        plan.swarm.max_workers = DEFAULT_MAX_WORKERS + 1;
        assert!(plan.validate().is_err());
    }

    #[test]
    fn rejects_incomplete_parity_policy() {
        let mut plan = SuperReasoningPlan::default_megaproject_plan("mission_1", "objective");
        let parity = plan.parity.as_mut().unwrap();
        parity.reference_command.clear();
        assert!(plan.validate().is_err());
    }
}
