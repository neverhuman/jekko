use serde::{Deserialize, Serialize};

use super::PortTargetRequest;

/// Phase state persisted for crash-safe resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortPhaseStatus {
    /// Stage or phase is being drafted.
    Drafting,
    /// Ordered plan exists.
    Planned,
    /// Workers are building task slices.
    Building,
    /// Proof lanes are running.
    Verifying,
    /// Cross-phase integration is being repaired.
    Healing,
    /// Parity lab is running.
    Parity,
    /// Phase is complete.
    Complete,
    /// Human or budget blocker.
    Blocked,
    /// Repeated failure parked the phase.
    Quarantined,
}

pub use PortPhaseStatus as PhaseStatus;

impl PortPhaseStatus {
    /// Whether this status is terminal for autonomous progression.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Blocked | Self::Quarantined)
    }
}

/// Master task state persisted for crash-safe resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasterTaskStatus {
    /// Task is queued.
    Queued,
    /// Task has a worker assignment.
    Assigned,
    /// Worker is running.
    Running,
    /// Proof lane failed.
    ProofFailed,
    /// Jankurai audit failed.
    AuditFailed,
    /// Worker branch merged.
    Merged,
    /// Worker changes rolled back.
    RolledBack,
    /// Repeated failure parked the task.
    Quarantined,
    /// Task is complete.
    Done,
}

impl MasterTaskStatus {
    /// Whether this task may be assigned to a worker.
    pub fn is_assignable(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::ProofFailed | Self::AuditFailed | Self::RolledBack
        )
    }
}

/// One stage in the generated master plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortStage {
    /// Stage id.
    pub id: String,
    /// Stage order.
    pub ordinal: usize,
    /// Human-readable name.
    pub name: String,
    /// Stage objective.
    pub objective: String,
    /// Current status.
    pub status: PhaseStatus,
    /// Stage dependencies by id.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Optional parallel group id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_group: Option<String>,
    /// Stage-level bounded write scope.
    #[serde(default)]
    pub write_scope: Vec<String>,
    /// Proof lanes required for stage signoff.
    #[serde(default)]
    pub proof_lanes: Vec<String>,
    /// Evidence artifacts required for signoff.
    #[serde(default)]
    pub signoff_evidence: Vec<String>,
}

/// One task in the generated master plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMasterTask {
    /// Task id.
    pub id: String,
    /// Owning stage id.
    pub stage_id: String,
    /// Task title.
    pub title: String,
    /// Task kind.
    #[serde(default = "default_task_kind")]
    pub task_kind: String,
    /// Risk level.
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
    /// Declared write scope.
    pub write_scope: Vec<String>,
    /// Whether write scope is bounded and non-empty.
    #[serde(default = "default_true")]
    pub bounded_write_scope: bool,
    /// Task dependencies by id.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Proof command or lane.
    pub proof_lane: String,
    /// Evidence required before done.
    #[serde(default)]
    pub done_evidence: Vec<String>,
    /// Memory scope for durable writes.
    #[serde(default = "default_memory_scope")]
    pub memory_scope: String,
    /// Check generated-zone boundaries before write.
    #[serde(default = "default_true")]
    pub generated_zone_boundary_checks: bool,
    /// Current task status.
    pub status: MasterTaskStatus,
}

/// Deterministic starter plan used before model-backed phase finalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMasterPlan {
    /// Captured target request.
    pub target: PortTargetRequest,
    /// Ordered stages.
    pub stages: Vec<PortStage>,
    /// Ordered master tasks.
    pub tasks: Vec<PortMasterTask>,
}

fn default_task_kind() -> String {
    "implementation".to_string()
}

fn default_risk_level() -> String {
    "medium".to_string()
}

pub(super) fn default_memory_scope() -> String {
    "run".to_string()
}

fn default_true() -> bool {
    true
}
