use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zyal_core::{MemoryKind, MemoryPromotionStatus};

use super::EvidenceLevel;

/// Durable memory write candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCapsule {
    /// Capsule id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Source artifact id.
    pub artifact_id: String,
    /// Memory scope.
    pub scope: String,
    /// `verified` or `rejected`.
    pub status: String,
    /// Stored summary.
    pub summary: String,
    /// Evidence strength.
    pub evidence_level: EvidenceLevel,
    /// Confidence.
    pub confidence: f64,
    /// Structured payload.
    #[serde(default)]
    pub payload_json: Value,
    /// Stable content hash.
    pub content_hash: String,
    /// Memory subfamily — Episodic / Semantic / Procedural / Negative.
    /// Defaults to `Semantic` for new capsules; the Memory Curator picks the
    /// right kind based on what the capsule encodes.
    #[serde(default)]
    pub memory_kind: MemoryKind,
    /// Promotion lifecycle stage. Capsules start `Scratch` and must be
    /// promoted by the Verifier / Reducer via [`Self::promote`].
    #[serde(default)]
    pub promotion_status: MemoryPromotionStatus,
    /// Human-readable claim — what this capsule actually asserts. Distinct
    /// from `summary` (free-form narrative) and `payload_json` (structured
    /// evidence). Phase E2's retrieval injects `claim_text` into prompts.
    #[serde(default)]
    pub claim_text: String,
    /// Role that approved promotion (`"verifier"` or `"reducer"`). `None`
    /// until [`Self::promote`] succeeds.
    #[serde(default)]
    pub approved_by_role: Option<String>,
}

impl MemoryCapsule {
    /// Capsule has been explicitly verified or rejected by the Verifier /
    /// Reducer lane. Required gate for any permanent write.
    pub fn is_verified_or_rejected(&self) -> bool {
        matches!(self.status.as_str(), "verified" | "rejected")
    }

    /// Capsule's evidence reaches `ExternalGrounding` or stronger — i.e. it
    /// references source / log / code / executable proof, not just internal
    /// model consistency.
    pub fn has_grounded_evidence(&self) -> bool {
        self.evidence_level >= EvidenceLevel::ExternalGrounding
    }

    /// Capsule names a source artifact, so its provenance can be audited.
    pub fn has_nonempty_provenance(&self) -> bool {
        !self.artifact_id.trim().is_empty()
    }

    /// Eligible for permanent memory write. Equivalent to the conjunction of
    /// the three predicates above; the split exists so callers can produce
    /// targeted error messages about which gate failed.
    pub fn can_write_permanent(&self) -> bool {
        self.is_verified_or_rejected()
            && self.has_grounded_evidence()
            && self.has_nonempty_provenance()
    }

    /// Advance the promotion lifecycle by exactly one step. Enforces:
    ///   Scratch → RunOnly → ProjectOnly → Global
    /// Returns an error on regression or any skip. Stamps `approved_by_role`
    /// so audits can see who promoted the capsule.
    ///
    /// Scratch → RunOnly additionally requires [`Self::can_write_permanent`]
    /// (verifier signoff + grounded evidence + provenance).
    pub fn promote(&mut self, target: MemoryPromotionStatus, by_role: &str) -> Result<()> {
        let by_role = by_role.trim();
        if by_role.is_empty() {
            bail!("promote: by_role must be non-empty (e.g. \"verifier\" / \"reducer\")");
        }
        use MemoryPromotionStatus::*;
        let next_after = |current: MemoryPromotionStatus| match current {
            Scratch => Some(RunOnly),
            RunOnly => Some(ProjectOnly),
            ProjectOnly => Some(Global),
            Global => None,
        };
        let expected = next_after(self.promotion_status).ok_or_else(|| {
            anyhow::anyhow!("promote: capsule already at Global, no further promotion")
        })?;
        if target != expected {
            bail!(
                "promote: illegal transition {:?} → {:?} (only {:?} is allowed)",
                self.promotion_status,
                target,
                expected
            );
        }
        if self.promotion_status == Scratch && !self.can_write_permanent() {
            bail!(
                "promote: Scratch → RunOnly requires verifier signoff (status verified/rejected, \
                 evidence_level >= ExternalGrounding, non-empty artifact_id)"
            );
        }
        self.promotion_status = target;
        self.approved_by_role = Some(by_role.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod memory_helpers_tests {
    use super::*;

    fn capsule(status: &str, level: EvidenceLevel, artifact_id: &str) -> MemoryCapsule {
        MemoryCapsule {
            id: "c1".to_string(),
            run_id: "r1".to_string(),
            artifact_id: artifact_id.to_string(),
            scope: "task".to_string(),
            status: status.to_string(),
            summary: String::new(),
            evidence_level: level,
            confidence: 0.5,
            payload_json: Value::Null,
            content_hash: String::new(),
            memory_kind: MemoryKind::Semantic,
            promotion_status: MemoryPromotionStatus::Scratch,
            claim_text: String::new(),
            approved_by_role: None,
        }
    }

    #[test]
    fn promote_walks_the_lifecycle_in_order() {
        let mut c = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        c.promote(MemoryPromotionStatus::RunOnly, "verifier")
            .unwrap();
        assert_eq!(c.promotion_status, MemoryPromotionStatus::RunOnly);
        assert_eq!(c.approved_by_role.as_deref(), Some("verifier"));
        c.promote(MemoryPromotionStatus::ProjectOnly, "reducer")
            .unwrap();
        assert_eq!(c.promotion_status, MemoryPromotionStatus::ProjectOnly);
        c.promote(MemoryPromotionStatus::Global, "verifier")
            .unwrap();
        assert_eq!(c.promotion_status, MemoryPromotionStatus::Global);
    }

    #[test]
    fn promote_rejects_regression() {
        let mut c = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        c.promote(MemoryPromotionStatus::RunOnly, "verifier")
            .unwrap();
        let err = c
            .promote(MemoryPromotionStatus::Scratch, "verifier")
            .unwrap_err();
        assert!(err.to_string().contains("illegal transition"));
    }

    #[test]
    fn promote_rejects_skipping_steps() {
        let mut c = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        let err = c
            .promote(MemoryPromotionStatus::Global, "verifier")
            .unwrap_err();
        assert!(err.to_string().contains("illegal transition"));
    }

    #[test]
    fn promote_scratch_to_runonly_requires_write_gate() {
        // Status candidate => can_write_permanent false => Scratch can't advance.
        let mut c = capsule("candidate", EvidenceLevel::ExternalGrounding, "a1");
        let err = c
            .promote(MemoryPromotionStatus::RunOnly, "verifier")
            .unwrap_err();
        assert!(err.to_string().contains("verifier signoff"));
    }

    #[test]
    fn promote_at_global_returns_error() {
        let mut c = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        for next in [
            MemoryPromotionStatus::RunOnly,
            MemoryPromotionStatus::ProjectOnly,
            MemoryPromotionStatus::Global,
        ] {
            c.promote(next, "verifier").unwrap();
        }
        let err = c
            .promote(MemoryPromotionStatus::Global, "verifier")
            .unwrap_err();
        assert!(err.to_string().contains("already at Global"));
    }

    #[test]
    fn promote_requires_nonempty_by_role() {
        let mut c = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        let err = c.promote(MemoryPromotionStatus::RunOnly, "  ").unwrap_err();
        assert!(err.to_string().contains("by_role must be non-empty"));
    }

    #[test]
    fn write_gate_requires_all_three() {
        let ok = capsule("verified", EvidenceLevel::ExternalGrounding, "a1");
        assert!(ok.is_verified_or_rejected());
        assert!(ok.has_grounded_evidence());
        assert!(ok.has_nonempty_provenance());
        assert!(ok.can_write_permanent());
    }

    #[test]
    fn missing_provenance_blocks_write() {
        let c = capsule("verified", EvidenceLevel::ExternalGrounding, "   ");
        assert!(!c.has_nonempty_provenance());
        assert!(!c.can_write_permanent());
    }

    #[test]
    fn weak_evidence_blocks_write() {
        let c = capsule("verified", EvidenceLevel::IndependentAgreement, "a1");
        assert!(!c.has_grounded_evidence());
        assert!(!c.can_write_permanent());
    }

    #[test]
    fn candidate_status_blocks_write() {
        let c = capsule("candidate", EvidenceLevel::Executable, "a1");
        assert!(!c.is_verified_or_rejected());
        assert!(!c.can_write_permanent());
    }
}

/// Per-model reliability accumulator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelReliability {
    /// Model id.
    pub model_id: String,
    /// Role or task kind.
    pub role: String,
    /// Task kind.
    pub task_kind: String,
    /// Success count.
    pub success_count: u64,
    /// Failure count.
    pub failure_count: u64,
    /// Winner count.
    pub winner_count: u64,
    /// Total latency.
    pub total_latency_ms: u64,
    /// Total cost.
    pub total_cost_usd: f64,
    /// Derived score.
    pub score: f64,
}

impl ModelReliability {
    /// Construct an empty accumulator.
    pub fn new(
        model_id: impl Into<String>,
        role: impl Into<String>,
        task_kind: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            role: role.into(),
            task_kind: task_kind.into(),
            success_count: 0,
            failure_count: 0,
            winner_count: 0,
            total_latency_ms: 0,
            total_cost_usd: 0.0,
            score: 0.0,
        }
    }

    /// Update counts from one outcome.
    pub fn record(&mut self, success: bool, winner: bool, latency_ms: u64, cost_usd: f64) {
        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        if winner {
            self.winner_count += 1;
        }
        self.total_latency_ms = self.total_latency_ms.saturating_add(latency_ms);
        self.total_cost_usd += cost_usd.max(0.0);
        self.score = self.compute_score();
    }

    fn compute_score(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.0;
        }
        let success_rate = self.success_count as f64 / total as f64;
        let winner_bonus = self.winner_count as f64 / total as f64 * 0.15;
        (success_rate + winner_bonus).clamp(0.0, 1.0)
    }
}
