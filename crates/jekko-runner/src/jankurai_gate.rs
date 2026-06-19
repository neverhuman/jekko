//! Jankurai audit gate policy for port checkpoints.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Jankurai audit counters relevant to checkpoint gating.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AuditSnapshot {
    /// Audit score.
    pub score: f64,
    /// Hard finding count.
    pub hard_findings: usize,
    /// Cap count.
    pub caps: usize,
}

/// Gate policy comparing a baseline and current audit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JankuraiGatePolicy {
    /// Maximum allowed new hard findings.
    pub max_new_hard_findings: isize,
    /// Maximum allowed score drop.
    pub max_score_drop: f64,
    /// Whether cap regressions fail the gate.
    pub block_cap_regression: bool,
}

impl Default for JankuraiGatePolicy {
    fn default() -> Self {
        Self {
            max_new_hard_findings: 0,
            max_score_drop: 0.0,
            block_cap_regression: true,
        }
    }
}

/// Check an audit snapshot against the baseline.
pub fn check_gate(
    baseline: AuditSnapshot,
    current: AuditSnapshot,
    policy: JankuraiGatePolicy,
) -> Result<()> {
    let hard_delta = current.hard_findings as isize - baseline.hard_findings as isize;
    let score_drop = baseline.score - current.score;
    let cap_delta = current.caps as isize - baseline.caps as isize;
    let mut errors = Vec::new();
    if hard_delta > policy.max_new_hard_findings {
        errors.push(format!("new hard findings: {hard_delta}"));
    }
    if score_drop > policy.max_score_drop {
        errors.push(format!("score drop: {score_drop:.2}"));
    }
    if policy.block_cap_regression && cap_delta > 0 {
        errors.push(format!("cap regression: {cap_delta}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}

/// Canonical audit program recorded in receipts.
pub const CANONICAL_AUDIT_PROGRAM: &str = "rtk";

/// Canonical audit arguments recorded in receipts.
pub const CANONICAL_AUDIT_ARGS: &[&str] = &[
    "jankurai",
    "audit",
    ".",
    "--mode",
    "advisory",
    "--json",
    ".jankurai/repo-score.json",
    "--md",
    ".jankurai/repo-score.md",
];

/// Canonical audit command recorded in receipts.
pub fn canonical_audit_command() -> Vec<&'static str> {
    std::iter::once(CANONICAL_AUDIT_PROGRAM)
        .chain(CANONICAL_AUDIT_ARGS.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_when_score_and_hard_counts_do_not_regress() {
        check_gate(
            AuditSnapshot {
                score: 90.0,
                hard_findings: 1,
                caps: 0,
            },
            AuditSnapshot {
                score: 91.0,
                hard_findings: 1,
                caps: 0,
            },
            JankuraiGatePolicy::default(),
        )
        .unwrap();
    }

    #[test]
    fn fails_on_hard_score_and_cap_regression() {
        let err = check_gate(
            AuditSnapshot {
                score: 90.0,
                hard_findings: 1,
                caps: 0,
            },
            AuditSnapshot {
                score: 89.0,
                hard_findings: 2,
                caps: 1,
            },
            JankuraiGatePolicy::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("hard"));
        assert!(err.contains("score"));
        assert!(err.contains("cap"));
    }
}
