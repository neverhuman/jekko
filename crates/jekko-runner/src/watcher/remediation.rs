//! Rule-based remediation engine.
//!
//! Reads a [`WatcherSnapshot`] (built by [`super::metrics::fold_events`]) and
//! emits zero or more [`RemediationAction`]s — pure decisions, no I/O. The
//! caller (G3's `jekko watch` CLI or the orchestrator's auto-remediation
//! loop) is responsible for translating actions into events + side effects.
//!
//! Rules are intentionally simple and easy to add. Add a new variant to
//! [`RemediationRule`] + a branch in [`detect_and_remediate`].

use std::collections::BTreeMap;

use super::metrics::WatcherSnapshot;

/// One rule the engine checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemediationRule {
    /// No progress event in the last N seconds — likely a stalled worker.
    StallDetected,
    /// Provider error rate over a threshold; write a Negative-memory capsule
    /// so future runs avoid the pattern.
    ProviderErrorBurst,
    /// Parity gaps growing across the last 3 observations rather than
    /// shrinking — escalate to a Critique lane.
    ParityGapsGrowing,
    /// Jankurai hard-findings count increased mid-run — block phase signoff.
    JankuraiRegression,
}

/// A concrete remediation decision. The caller maps these to events and
/// side effects (writing capsules, restarting workers, emitting alerts).
#[derive(Clone, Debug, PartialEq)]
pub struct RemediationAction {
    pub rule: RemediationRule,
    /// One-line summary the caller can log or surface in the dashboard.
    pub summary: String,
    /// Free-form key/value detail for the emitted event payload.
    pub detail: BTreeMap<String, String>,
}

/// Apply every rule to the snapshot. Returns the actions that fired (may be
/// empty). The caller decides whether to act, log only, or escalate.
///
/// Arguments:
/// - `snap` — the current snapshot.
/// - `now_ts` — current epoch seconds. Stall detection compares this against
///   `snap.last_progress_ts`.
/// - `stall_threshold_secs` — how long without progress before
///   [`RemediationRule::StallDetected`] fires (default in callers: 300).
/// - `error_rate_threshold` — `0.0..=1.0`; provider error rate above this
///   triggers [`RemediationRule::ProviderErrorBurst`] when there are enough
///   samples (≥ 20 attempts).
/// - `prior_gaps_open` — `parity_gaps_open` from the snapshot three ticks
///   ago. Pass `None` when there's no history yet. Used to detect
///   [`RemediationRule::ParityGapsGrowing`].
/// - `prior_hard_findings` — `last_jankurai_hard_findings` from the previous
///   audit observation. Pass `None` until the second audit lands. When the
///   current value exceeds the prior, [`RemediationRule::JankuraiRegression`]
///   fires.
pub fn detect_and_remediate(
    snap: &WatcherSnapshot,
    now_ts: u64,
    stall_threshold_secs: u64,
    error_rate_threshold: f64,
    prior_gaps_open: Option<i64>,
    prior_hard_findings: Option<i64>,
) -> Vec<RemediationAction> {
    let mut out = Vec::new();

    // StallDetected — only when we've seen at least one progress event.
    if let Some(last) = snap.last_progress_ts {
        if !snap.finished && now_ts >= last && (now_ts - last) >= stall_threshold_secs {
            let elapsed = now_ts - last;
            out.push(RemediationAction {
                rule: RemediationRule::StallDetected,
                summary: format!("no progress for {elapsed}s (threshold {stall_threshold_secs}s)"),
                detail: BTreeMap::from([
                    ("elapsed_secs".to_string(), elapsed.to_string()),
                    (
                        "threshold_secs".to_string(),
                        stall_threshold_secs.to_string(),
                    ),
                ]),
            });
        }
    }

    // ProviderErrorBurst — needs at least 20 attempts to make a statement.
    if snap.model_attempts >= 20 && snap.error_rate() > error_rate_threshold {
        let worst = snap
            .errors_by_provider
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(provider, count)| (provider.clone(), *count));
        let detail = match &worst {
            Some((provider, count)) => BTreeMap::from([
                (
                    "error_rate".to_string(),
                    format!("{:.2}", snap.error_rate()),
                ),
                ("attempts".to_string(), snap.model_attempts.to_string()),
                ("worst_provider".to_string(), provider.clone()),
                ("worst_provider_errors".to_string(), count.to_string()),
            ]),
            None => BTreeMap::from([
                (
                    "error_rate".to_string(),
                    format!("{:.2}", snap.error_rate()),
                ),
                ("attempts".to_string(), snap.model_attempts.to_string()),
            ]),
        };
        out.push(RemediationAction {
            rule: RemediationRule::ProviderErrorBurst,
            summary: match &worst {
                Some((provider, _)) => format!(
                    "error rate {:.0}% over {} attempts (worst: {})",
                    snap.error_rate() * 100.0,
                    snap.model_attempts,
                    provider
                ),
                None => format!(
                    "error rate {:.0}% over {} attempts",
                    snap.error_rate() * 100.0,
                    snap.model_attempts
                ),
            },
            detail,
        });
    }

    // ParityGapsGrowing — only when we have a prior observation AND the
    // current count is strictly higher.
    if let Some(prior) = prior_gaps_open {
        if snap.parity_gaps_open > prior {
            out.push(RemediationAction {
                rule: RemediationRule::ParityGapsGrowing,
                summary: format!(
                    "parity gaps grew {prior} -> {} (expected to shrink)",
                    snap.parity_gaps_open
                ),
                detail: BTreeMap::from([
                    ("prior_open".to_string(), prior.to_string()),
                    (
                        "current_open".to_string(),
                        snap.parity_gaps_open.to_string(),
                    ),
                ]),
            });
        }
    }

    // JankuraiRegression — current > prior implies new hard findings.
    if let (Some(prior), Some(current)) = (prior_hard_findings, snap.last_jankurai_hard_findings) {
        if current > prior {
            out.push(RemediationAction {
                rule: RemediationRule::JankuraiRegression,
                summary: format!(
                    "jankurai hard findings rose {prior} -> {current} — phase signoff should block"
                ),
                detail: BTreeMap::from([
                    ("prior".to_string(), prior.to_string()),
                    ("current".to_string(), current.to_string()),
                ]),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stall_fires_only_after_threshold() {
        let snap = WatcherSnapshot {
            last_progress_ts: Some(100),
            ..Default::default()
        };
        // 200s elapsed, threshold 300 — no fire.
        let actions = detect_and_remediate(&snap, 300, 300, 0.5, None, None);
        assert!(actions.is_empty());
        // 400s elapsed, threshold 300 — fire.
        let actions = detect_and_remediate(&snap, 500, 300, 0.5, None, None);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].rule, RemediationRule::StallDetected);
    }

    #[test]
    fn stall_does_not_fire_after_run_finished() {
        let snap = WatcherSnapshot {
            last_progress_ts: Some(100),
            finished: true,
            ..Default::default()
        };
        let actions = detect_and_remediate(&snap, 9999, 300, 0.5, None, None);
        assert!(actions
            .iter()
            .all(|a| a.rule != RemediationRule::StallDetected));
    }

    #[test]
    fn provider_error_burst_requires_min_attempts() {
        let mut snap = WatcherSnapshot {
            model_attempts: 5,
            model_failures: 4, // 80% error rate, but only 5 attempts
            ..Default::default()
        };
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, None, None);
        assert!(actions
            .iter()
            .all(|a| a.rule != RemediationRule::ProviderErrorBurst));
        snap.model_attempts = 20;
        snap.model_failures = 11; // 55%
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, None, None);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].rule, RemediationRule::ProviderErrorBurst);
    }

    #[test]
    fn parity_gaps_growing_fires_only_on_increase() {
        let snap = WatcherSnapshot {
            parity_gaps_open: 5,
            ..Default::default()
        };
        // No prior — no fire.
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, None, None);
        assert!(actions
            .iter()
            .all(|a| a.rule != RemediationRule::ParityGapsGrowing));
        // Prior 3, current 5 — fire.
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, Some(3), None);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].rule, RemediationRule::ParityGapsGrowing);
        // Prior 8, current 5 — no fire (shrinking is healthy).
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, Some(8), None);
        assert!(actions.is_empty());
    }

    #[test]
    fn jankurai_regression_fires_on_increased_hard_findings() {
        let snap = WatcherSnapshot {
            last_jankurai_hard_findings: Some(5),
            ..Default::default()
        };
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, None, Some(3));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].rule, RemediationRule::JankuraiRegression);
        // Hard findings unchanged — no fire.
        let actions = detect_and_remediate(&snap, 0, 600, 0.5, None, Some(5));
        assert!(actions.is_empty());
    }
}
