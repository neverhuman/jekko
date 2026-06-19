//! Anti-Goodhart guards (M11-cont).
//!
//! A KPI optimized as a target stops measuring what it meant to. These guards
//! keep a hidden, SALT-SECRET holdout of cases that never feed back into the
//! optimization, then alert when the proxy metric improves but the holdout
//! (hidden truth) does not track it — i.e. the proxy is being gamed.

use serde::{Deserialize, Serialize};

use crate::hashing::sha256_hex;

/// How a guard watches a KPI for Goodharting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardMode {
    /// Run a shadow metric alongside the live one.
    Shadow,
    /// Reserve a hidden set of cases that never feed back.
    Holdout,
    /// Alert when independent judges disagree.
    Disagreement,
    /// Watch the residual between proxy and outcome.
    Residual,
    /// Alert when the proxy drifts away from the true objective.
    ProxyDrift,
    /// Alert on input/output distribution shift.
    DistributionShift,
    /// Alert when a previously-winning judgment flips.
    JudgeFlip,
}

/// Whether `case_id` belongs to the hidden holdout. Deterministic + replayable,
/// but the `salt` is SECRET (never serialized into events/receipts/prompts) so a
/// model cannot learn which cases are held out and game only the rest. A case is
/// in the holdout iff `sha256(salt|case_id)` maps into the first `holdout_pct`%
/// of the bucket space.
pub fn in_holdout(salt: &str, case_id: &str, holdout_pct: u8) -> bool {
    let pct = holdout_pct.min(100) as u32;
    if pct == 0 {
        return false;
    }
    let digest = sha256_hex(format!("{salt}|{case_id}").as_bytes());
    let bucket = u32::from_str_radix(&digest[..8], 16).unwrap_or(0) % 100;
    bucket < pct
}

/// Goodhart alert: the PROXY metric improved by `proxy_delta` but the holdout
/// (hidden truth) only moved by `holdout_delta`. When the proxy outruns the
/// holdout by more than `tolerance`, the proxy is likely being gamed.
pub fn proxy_drift_alert(proxy_delta: f64, holdout_delta: f64, tolerance: f64) -> bool {
    proxy_delta.is_finite()
        && holdout_delta.is_finite()
        && tolerance >= 0.0
        && proxy_delta - holdout_delta > tolerance
}

/// The global KPI as a weighted composite of `(value, weight)` components,
/// normalized by total weight. Non-finite / non-positive-weight components are
/// dropped; `None` when no component contributes. This is the single number a
/// guard watches against its holdout — a weighted composite is harder to game
/// than any single proxy.
pub fn weighted_kpi(components: &[(f64, f64)]) -> Option<f64> {
    let mut num = 0.0;
    let mut den = 0.0;
    for &(value, weight) in components {
        if value.is_finite() && weight.is_finite() && weight > 0.0 {
            num += value * weight;
            den += weight;
        }
    }
    (den > 0.0).then_some(num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_is_deterministic_and_roughly_proportional() {
        // Deterministic: same (salt, case) → same membership.
        assert_eq!(
            in_holdout("s3cr3t", "case-42", 20),
            in_holdout("s3cr3t", "case-42", 20)
        );
        // 0% holds out nothing.
        assert!(!in_holdout("s", "anything", 0));
        // A different salt reshuffles membership (anti-Goodhart: secret salt).
        let a: usize = (0..1000)
            .filter(|i| in_holdout("saltA", &format!("c{i}"), 30))
            .count();
        let b: usize = (0..1000)
            .filter(|i| in_holdout("saltB", &format!("c{i}"), 30))
            .count();
        // Each ~30% of 1000, but the SETS differ (different salt).
        assert!((200..400).contains(&a) && (200..400).contains(&b));
        let overlap: usize = (0..1000)
            .filter(|i| {
                in_holdout("saltA", &format!("c{i}"), 30)
                    && in_holdout("saltB", &format!("c{i}"), 30)
            })
            .count();
        assert!(overlap < a, "a secret salt must reshuffle the holdout set");
    }

    #[test]
    fn proxy_drift_fires_only_when_proxy_outruns_holdout() {
        // proxy up 0.20, holdout up 0.02 → gap 0.18 > tol 0.05 → alert
        assert!(proxy_drift_alert(0.20, 0.02, 0.05));
        // proxy + holdout both up together → no alert (real improvement)
        assert!(!proxy_drift_alert(0.20, 0.19, 0.05));
        // NaN guarded
        assert!(!proxy_drift_alert(f64::NAN, 0.0, 0.05));
    }

    #[test]
    fn weighted_kpi_normalizes_and_drops_bad_components() {
        // (0.8*2 + 0.6*1) / (2+1) = 2.2/3
        let k = weighted_kpi(&[(0.8, 2.0), (0.6, 1.0)]).unwrap();
        assert!((k - 2.2 / 3.0).abs() < 1e-9);
        // NaN value + zero/negative weight are dropped.
        assert_eq!(
            weighted_kpi(&[(0.9, 1.0), (f64::NAN, 5.0), (0.1, 0.0)]).unwrap(),
            0.9
        );
        // no contributing component → None.
        assert!(weighted_kpi(&[(0.5, 0.0)]).is_none());
        assert!(weighted_kpi(&[]).is_none());
    }
}
