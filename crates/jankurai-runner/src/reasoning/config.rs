use serde::{Deserialize, Serialize};

use crate::port::MAX_PORT_WORKERS;

/// Default confidence cap for unsupported or non-executable reasoning.
pub const DEFAULT_CONFIDENCE_CAP: f64 = 0.35;

/// Advanced reasoning runtime options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvancedReasoningConfig {
    /// Enable the advanced state machine.
    #[serde(default)]
    pub enabled: bool,
    /// Requested worker lanes. Clamped to ten.
    #[serde(default = "default_worker_cap")]
    pub worker_cap: usize,
    /// Maximum confidence without executable evidence.
    #[serde(default = "default_confidence_cap")]
    pub confidence_cap: f64,
    /// Store raw model reasoning text. Defaults false and should stay false.
    #[serde(default)]
    pub store_raw_reasoning: bool,
    /// Permit power models outside reducer/critic/escalation routes.
    #[serde(default)]
    pub allow_power_for_routine_roles: bool,
}

impl Default for AdvancedReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            worker_cap: default_worker_cap(),
            confidence_cap: DEFAULT_CONFIDENCE_CAP,
            store_raw_reasoning: false,
            allow_power_for_routine_roles: false,
        }
    }
}

impl AdvancedReasoningConfig {
    /// Return the effective worker cap enforced by the runtime.
    pub fn effective_worker_cap(&self) -> usize {
        self.worker_cap.clamp(1, MAX_PORT_WORKERS)
    }

    /// Return the effective confidence cap.
    pub fn effective_confidence_cap(&self) -> f64 {
        if self.confidence_cap.is_finite() {
            self.confidence_cap.clamp(0.0, DEFAULT_CONFIDENCE_CAP)
        } else {
            DEFAULT_CONFIDENCE_CAP
        }
    }
}

/// Role that produced a reasoning artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningRole {
    /// Frames the request and success criteria.
    Framer,
    /// Retrieves code, docs, memory, and parity context.
    Retriever,
    /// Proposes stages or phase slices.
    Planner,
    /// Builds one bounded implementation lane.
    Builder,
    /// Tries to falsify a candidate.
    Critic,
    /// Runs executable or source-grounded checks.
    Verifier,
    /// Reduces multiple candidates into a host-owned decision.
    Reducer,
    /// Curates durable memory after verification.
    MemoryCurator,
}

/// Reasoning artifact kind. Canonical enum lives in `zyal-core` and includes
/// 8 additional variants used by the Phase F super-agent kernel (`MacroPlan`,
/// `PhaseDag`, `FunctionGraph`, `ParityCase`, `PerfGap`, `SignoffReceipt`,
/// `Contradiction`, `ReducerDecision`). Aliased here so existing
/// `ReasoningArtifactKind::TaskContract` paths keep compiling.
pub use zyal_core::ArtifactKind as ReasoningArtifactKind;

/// Evidence strength. E4+ is executable enough to lift confidence caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    /// E0: unsupported model claim.
    Unsupported,
    /// E1: internally consistent only.
    InternalConsistency,
    /// E2: independent agreement.
    IndependentAgreement,
    /// E3: source/log/code grounded.
    ExternalGrounding,
    /// E4: executable verification.
    Executable,
    /// E5: survived adversarial review.
    AdversarialSurvival,
    /// E6: durable historical support.
    HistoricalDurability,
}

impl EvidenceLevel {
    /// Whether this level is executable or stronger.
    pub fn has_executable_evidence(self) -> bool {
        self >= Self::Executable
    }
}

fn default_worker_cap() -> usize {
    3
}

fn default_confidence_cap() -> f64 {
    DEFAULT_CONFIDENCE_CAP
}
