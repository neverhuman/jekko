//! Phase-shape types backing [`super::SuperReasoningPlan`].
//!
//! Split out of `super_reasoning/types.rs` to keep individual files under
//! the 500-LOC shape threshold (jankurai HLT-001:shape). All items are
//! re-exported by the parent module via `pub use phase::*;`, so external
//! callers continue to import from `crate::daemon::super_reasoning::Foo`.

use serde::{Deserialize, Serialize};

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
pub enum ReasoningWriteScope {
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
pub enum ReasoningPhaseSignoffMode {
    /// Gate-only signoff.
    Automatic,
    /// Reviewer agent signoff.
    Reviewer,
    /// Human signoff.
    Human,
    /// Evidence-only signoff.
    EvidenceOnly,
}

pub use ReasoningPhaseSignoffMode as PhaseSignoffMode;
pub use ReasoningWriteScope as WriteScope;

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
