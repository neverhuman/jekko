use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::model_client::CredentialSourcePolicy;
use crate::port::{
    MasterTaskStatus, PhaseStatus, PortMasterPlan, PortMasterTask, PortStage, PortTargetRequest,
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

struct StageTemplate {
    slug: &'static str,
    name: &'static str,
    objective: &'static str,
    depends_on: &'static [&'static str],
}

/// Inline mirror of `jekko_runtime::daemon::super_reasoning::canonical_phases()`.
///
/// The runtime function is private and lives in a sibling crate, so the names
/// are kept in sync here. If the canonical list shifts upstream, update this
/// table and the runtime in tandem.
fn canonical_stage_templates() -> Vec<StageTemplate> {
    vec![
        StageTemplate {
            slug: "source_of_truth",
            name: "Source of truth",
            objective: "Build the non-negotiable behavior, API, compatibility, and evidence ledger.",
            depends_on: &[],
        },
        StageTemplate {
            slug: "architecture_blueprint",
            name: "Architecture blueprint",
            objective: "Convert requirements into module boundaries, invariants, risk register, and implementation slices.",
            depends_on: &["source_of_truth"],
        },
        StageTemplate {
            slug: "repo_graph_bootstrap",
            name: "Repo graph bootstrap",
            objective: "Index functions, symbols, tests, call edges, dataflow hints, ownership, and blast radius.",
            depends_on: &["source_of_truth"],
        },
        StageTemplate {
            slug: "contracts_and_slices",
            name: "Contracts and slices",
            objective: "Produce task contracts, proof commands, fixture strategy, and independent slices.",
            depends_on: &["architecture_blueprint", "repo_graph_bootstrap"],
        },
        StageTemplate {
            slug: "parallel_subsystems",
            name: "Parallel subsystems",
            objective: "Implement independent verified slices in isolated worktrees with critic lanes.",
            depends_on: &["contracts_and_slices"],
        },
        StageTemplate {
            slug: "integration_fusion",
            name: "Integration fusion",
            objective: "Fuse verified subsystem work, resolve interface drift, and run integration proof lanes.",
            depends_on: &["parallel_subsystems"],
        },
        StageTemplate {
            slug: "parity_lab",
            name: "Parity lab",
            objective: "Create differential, golden, metamorphic, and fuzz parity harnesses.",
            depends_on: &["source_of_truth", "contracts_and_slices"],
        },
        StageTemplate {
            slug: "parity_gap_closure",
            name: "Parity gap closure",
            objective: "Close blocking parity gaps using the gap ledger and regression proofs.",
            depends_on: &["integration_fusion", "parity_lab"],
        },
        StageTemplate {
            slug: "performance_closure",
            name: "Performance closure",
            objective: "Benchmark reference versus candidate and close hot-path gaps without weakening parity.",
            depends_on: &["parity_gap_closure"],
        },
        StageTemplate {
            slug: "hardening_security",
            name: "Hardening and security",
            objective: "Run fuzzing, stress, recovery, race, security, and fault-injection proof lanes.",
            depends_on: &["parity_gap_closure"],
        },
        StageTemplate {
            slug: "docs_release_ops",
            name: "Docs, release, and operations",
            objective: "Produce docs, CI gates, migration notes, release checklist, and operational runbooks.",
            depends_on: &["performance_closure", "hardening_security"],
        },
        StageTemplate {
            slug: "final_signoff",
            name: "Final signoff",
            objective: "Aggregate receipts, rerun full proofs, ensure clean tree, and require final approval.",
            depends_on: &["docs_release_ops"],
        },
    ]
}

fn stage_id_for(idx: usize, slug: &str) -> String {
    format!("stage-{:02}-{}", idx + 1, slug)
}

fn write_scope_for_stage(slug: &str) -> Vec<String> {
    match slug {
        "source_of_truth" | "architecture_blueprint" => vec![
            "docs/**".to_string(),
            "target/zyal/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        "repo_graph_bootstrap" => vec![
            "target/zyal/repo-graph/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        "parity_lab" | "parity_gap_closure" => vec![
            "tests/parity/**".to_string(),
            "target/zyal/parity/**".to_string(),
            "crates/**/tests/**".to_string(),
        ],
        "performance_closure" => vec![
            "benches/**".to_string(),
            "target/zyal/parity/**".to_string(),
            "target/zyal/perf/**".to_string(),
        ],
        "final_signoff" | "docs_release_ops" => vec![
            "target/zyal/**".to_string(),
            "docs/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        _ => vec![
            "src/**".to_string(),
            "crates/**".to_string(),
            "tests/**".to_string(),
            "target/zyal/**".to_string(),
        ],
    }
}

/// Build a generic 9-12 stage super-reasoning master plan from a target
/// request.
///
/// The names mirror `jekko_runtime::daemon::super_reasoning::canonical_phases()`
/// so a runtime kicked off from this plan finds the phase ids it expects.
pub fn draft_super_master_plan(target: &PortTargetRequest) -> PortMasterPlan {
    draft_super_master_plan_with_config(target, &SuperReasoningConfig::default())
}

/// Like [`draft_super_master_plan`] but lets the caller override the
/// macro-stage target (clamped to 9..=12).
pub fn draft_super_master_plan_with_config(
    target: &PortTargetRequest,
    config: &SuperReasoningConfig,
) -> PortMasterPlan {
    let stage_count = config.effective_stage_target();
    let templates = canonical_stage_templates();
    let target_name = target.target.clone();
    let replacement = target.replacement.clone();

    // Slug -> stage_id table for dependency rewriting.
    let mut id_table = BTreeMap::new();
    for (idx, template) in templates.iter().take(stage_count).enumerate() {
        id_table.insert(template.slug, stage_id_for(idx, template.slug));
    }

    let mut stages = Vec::with_capacity(stage_count);
    let mut tasks = Vec::with_capacity(stage_count * 2);
    for (idx, template) in templates.iter().take(stage_count).enumerate() {
        let stage_id = id_table[template.slug].clone();
        let dependencies: Vec<String> = template
            .depends_on
            .iter()
            .filter_map(|slug| id_table.get(slug).cloned())
            .collect();
        let write_scope = write_scope_for_stage(template.slug);

        stages.push(PortStage {
            id: stage_id.clone(),
            ordinal: idx + 1,
            name: template.name.to_string(),
            objective: format!(
                "{} for {} -> {}. {}",
                template.name, target_name, replacement, template.objective
            ),
            status: if idx == 0 {
                PhaseStatus::Drafting
            } else {
                PhaseStatus::Planned
            },
            dependencies: dependencies.clone(),
            parallel_group: Some(format!("group-{:02}", idx + 1)),
            write_scope: write_scope.clone(),
            proof_lanes: vec!["just zyal-port-fast".to_string()],
            signoff_evidence: vec![
                "proof_receipt".to_string(),
                "replay_receipt".to_string(),
                "parity_receipt".to_string(),
            ],
        });

        let exec_task_id = format!("task-{:02}-{}-execute", idx + 1, template.slug);
        let signoff_task_id = format!("task-{:02}-{}-signoff", idx + 1, template.slug);
        tasks.push(PortMasterTask {
            id: exec_task_id.clone(),
            stage_id: stage_id.clone(),
            title: format!("Execute {} for {}", template.name, replacement),
            task_kind: "implementation".to_string(),
            risk_level: "medium".to_string(),
            write_scope: write_scope.clone(),
            bounded_write_scope: true,
            dependencies: Vec::new(),
            proof_lane: "just zyal-port-fast".to_string(),
            done_evidence: vec!["tests_passed".to_string(), "replay_receipt".to_string()],
            memory_scope: "run".to_string(),
            generated_zone_boundary_checks: true,
            status: MasterTaskStatus::Queued,
        });
        tasks.push(PortMasterTask {
            id: signoff_task_id,
            stage_id,
            title: format!(
                "Sign off {} with reducer, verifier, Jankurai, and parity receipts",
                template.name
            ),
            task_kind: "signoff".to_string(),
            risk_level: "medium".to_string(),
            write_scope: vec![
                "target/zyal/**".to_string(),
                ".jankurai/**".to_string(),
                "tests/parity/**".to_string(),
            ],
            bounded_write_scope: true,
            dependencies: vec![exec_task_id],
            proof_lane: "just zyal-port-fast".to_string(),
            done_evidence: vec![
                "jankurai_gate_passed".to_string(),
                "parity_receipt".to_string(),
            ],
            memory_scope: "run".to_string(),
            generated_zone_boundary_checks: true,
            status: MasterTaskStatus::Queued,
        });
    }

    PortMasterPlan {
        target: target.clone(),
        stages,
        tasks,
    }
}

/// Validate the macro-plan shape before persisting or fusing work.
///
/// Checks: stage count in `[SUPER_STAGE_MIN, SUPER_STAGE_MAX]`, unique stage
/// ids, and acyclic stage-dependency graph.
pub fn validate_super_macro_plan(plan: &PortMasterPlan) -> Result<()> {
    let count = plan.stages.len();
    if !(SUPER_STAGE_MIN..=SUPER_STAGE_MAX).contains(&count) {
        return Err(anyhow!(
            "super reasoning macro plan must contain {SUPER_STAGE_MIN}-{SUPER_STAGE_MAX} stages, got {count}"
        ));
    }
    let mut ids = BTreeSet::new();
    for stage in &plan.stages {
        if !ids.insert(stage.id.as_str()) {
            return Err(anyhow!("duplicate macro-stage id {}", stage.id));
        }
    }
    for stage in &plan.stages {
        for dep in &stage.dependencies {
            if !ids.contains(dep.as_str()) {
                return Err(anyhow!(
                    "macro-stage {} has unknown dependency {}",
                    stage.id,
                    dep
                ));
            }
        }
    }
    ensure_acyclic_stages(plan)?;
    Ok(())
}

fn ensure_acyclic_stages(plan: &PortMasterPlan) -> Result<()> {
    let graph: BTreeMap<&str, Vec<&str>> = plan
        .stages
        .iter()
        .map(|stage| {
            (
                stage.id.as_str(),
                stage.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys().copied() {
        visit(node, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    node: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node) {
        return Err(anyhow!(
            "super reasoning macro plan has a stage dependency cycle through {node}"
        ));
    }
    for dep in graph.get(node).into_iter().flatten().copied() {
        if graph.contains_key(dep) {
            visit(dep, graph, visiting, visited)?;
        }
    }
    visiting.remove(node);
    visited.insert(node);
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_max_workers() -> usize {
    MAX_SUPERREASONING_WORKERS
}

fn default_macro_stage_target() -> usize {
    DEFAULT_SUPER_STAGE_TARGET
}

fn default_max_parallel_phases() -> usize {
    3
}

fn default_per_phase_worker_cap() -> usize {
    MAX_SUPERREASONING_WORKERS
}

fn default_memory_write_requires() -> Vec<String> {
    vec![
        "verified_or_rejected_status".to_string(),
        "source_artifact_hash".to_string(),
        "verifier_or_reducer_approval".to_string(),
        "no_raw_chain_of_thought".to_string(),
    ]
}

fn default_context_tokens() -> usize {
    24_000
}

fn default_promotion_threshold() -> f64 {
    0.75
}

fn default_retention_horizon() -> String {
    "7d".to_string()
}

fn default_graph_store() -> String {
    "sqlite".to_string()
}

fn default_graph_slice_node_budget() -> usize {
    256
}

fn default_required_case_tags() -> Vec<String> {
    vec!["required".to_string(), "approved".to_string()]
}

fn default_gap_task_prefix() -> String {
    "parity-gap".to_string()
}

fn default_p95_ratio() -> f64 {
    1.25
}

fn default_ramdisk_root() -> String {
    "/dev/shm/zyal".to_string()
}

fn default_run_root() -> String {
    ".zyal/runs/${run.id}".to_string()
}

fn default_worktree_root() -> String {
    ".zyal/worktrees/${run.id}".to_string()
}

fn default_worktree_pool_size() -> usize {
    MAX_SUPERREASONING_WORKERS
}

fn default_gc_after() -> String {
    "14d".to_string()
}
