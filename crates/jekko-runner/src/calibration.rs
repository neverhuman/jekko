//! Deterministic calibration of promotion-blend weights against objective
//! ground truth (U6).
//!
//! When an objective oracle is configured (U1), each generation yields a
//! labeled sample: the winner's self-reported `hero` score, the `verifier`
//! score, and whether the oracle passed. [`calibrate_blend`] fits how much each
//! component actually separates passing from failing outcomes, so the system
//! can learn which signal predicts ground truth instead of trusting fixed
//! hand-tuned weights.
//!
//! The result is **advisory**: it is emitted as a per-run artifact and intended
//! as a prior for future tuning. It deliberately does NOT mutate the live score
//! mid-run, preserving cogcore-style deterministic replay.

use serde::Serialize;

/// Fitted blend weights for the promotion-score components. `hero + verifier`
/// sum to 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BlendWeights {
    /// Weight for the model's self-reported hero score.
    pub hero: f64,
    /// Weight for the verifier score.
    pub verifier: f64,
    /// Number of labeled samples the fit was computed from.
    pub samples: usize,
}

impl BlendWeights {
    /// The normalized prior (the shipped 0.70 / 0.25 split, renormalized to sum
    /// to 1) used when there is not enough signal to calibrate.
    fn prior(samples: usize) -> Self {
        let total = 0.70 + 0.25;
        Self {
            hero: 0.70 / total,
            verifier: 0.25 / total,
            samples,
        }
    }
}

/// Mean separation of a component between passing and failing samples, clamped
/// to `[0, 1]`. A component whose score is higher on passing outcomes than on
/// failing ones separates well and earns more weight.
fn separation(samples: &[(f64, f64, bool)], component: impl Fn(&(f64, f64, bool)) -> f64) -> f64 {
    let (mut pass_sum, mut pass_n, mut fail_sum, mut fail_n) = (0.0, 0usize, 0.0, 0usize);
    for sample in samples {
        if sample.2 {
            pass_sum += component(sample);
            pass_n += 1;
        } else {
            fail_sum += component(sample);
            fail_n += 1;
        }
    }
    if pass_n == 0 || fail_n == 0 {
        return 0.0;
    }
    ((pass_sum / pass_n as f64) - (fail_sum / fail_n as f64)).clamp(0.0, 1.0)
}

/// Deterministically fit blend weights from labeled `(hero, verifier, passed)`
/// samples. Each component is weighted in proportion to how well it separates
/// passing from failing outcomes. Falls back to the normalized prior when there
/// are too few samples, only one outcome class, or no separation signal.
pub fn calibrate_blend(samples: &[(f64, f64, bool)]) -> BlendWeights {
    if samples.len() < 3 {
        return BlendWeights::prior(samples.len());
    }
    let hero_sep = separation(samples, |sample| sample.0);
    let verifier_sep = separation(samples, |sample| sample.1);
    let total = hero_sep + verifier_sep;
    if total <= f64::EPSILON {
        return BlendWeights::prior(samples.len());
    }
    BlendWeights {
        hero: hero_sep / total,
        verifier: verifier_sep / total,
        samples: samples.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_shift_to_the_component_that_predicts_ground_truth() {
        // Hero score cleanly separates pass (high) from fail (low); verifier is
        // noise (constant). The fit must weight hero far above verifier.
        let samples = vec![
            (0.9, 0.5, true),
            (0.85, 0.5, true),
            (0.2, 0.5, false),
            (0.15, 0.5, false),
        ];
        let weights = calibrate_blend(&samples);
        assert!(
            weights.hero > weights.verifier,
            "hero predicts the oracle, so it should dominate: {weights:?}"
        );
        assert!((weights.hero + weights.verifier - 1.0).abs() < 1e-9);
        assert_eq!(weights.samples, 4);
    }

    #[test]
    fn falls_back_to_prior_without_enough_signal() {
        // Too few samples.
        let few = calibrate_blend(&[(0.9, 0.5, true)]);
        assert_eq!(few, BlendWeights::prior(1));
        // Only one outcome class (all pass) -> no separation -> prior.
        let one_class = calibrate_blend(&[(0.9, 0.5, true), (0.8, 0.4, true), (0.7, 0.6, true)]);
        assert!(one_class.hero > one_class.verifier); // prior is hero-leaning
        assert!((one_class.hero + one_class.verifier - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fit_is_deterministic() {
        let samples = vec![
            (0.9, 0.7, true),
            (0.3, 0.6, false),
            (0.8, 0.65, true),
            (0.2, 0.55, false),
        ];
        assert_eq!(calibrate_blend(&samples), calibrate_blend(&samples));
    }
}
