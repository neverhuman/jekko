//! 10-axis pure-function scoring.
//!
//! Each scoring function takes a candidate `RecallResult` plus the fixture's
//! `Expected` and returns `Option<f32>` where:
//!   * `Some(s)` — the fixture exercised this axis; s ∈ [0.0, 1.0]
//!   * `None` — fixture doesn't test this axis; exclude from the average
//!
//! Per-axis averages are computed by `bin/bench.rs` over only the `Some`
//! contributions, so axes a fixture doesn't exercise don't inflate the score.

use crate::fixture::Expected;
use crate::{AxisScores, RecallResult};

fn answer_contains_all(out: &RecallResult, needles: &[&str]) -> bool {
    let lower = out.answer.to_lowercase();
    needles.iter().all(|n| lower.contains(&n.to_lowercase()))
}

fn answer_contains_none(out: &RecallResult, needles: &[&str]) -> bool {
    needles.iter().all(|n| !out.answer.contains(n))
}

fn used_ids_contains_all(out: &RecallResult, needles: &[&str]) -> bool {
    needles
        .iter()
        .all(|id| out.used_ids.iter().any(|u| u == id))
}

fn used_ids_contains_none(out: &RecallResult, needles: &[&str]) -> bool {
    needles
        .iter()
        .all(|id| !out.used_ids.iter().any(|u| u == id))
}

// ───────── axis functions: Option<f32> ─────────

pub fn correctness(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let has_constraint = !exp.must_contain.is_empty()
        || !exp.must_not_contain.is_empty()
        || !exp.must_include.is_empty()
        || !exp.must_exclude.is_empty();
    if !has_constraint {
        return None;
    }
    let mut hits = 0u32;
    let mut total = 0u32;
    if !exp.must_contain.is_empty() {
        total += 1;
        if answer_contains_all(out, exp.must_contain) {
            hits += 1;
        }
    }
    if !exp.must_not_contain.is_empty() {
        total += 1;
        if answer_contains_none(out, exp.must_not_contain) {
            hits += 1;
        }
    }
    if !exp.must_include.is_empty() {
        total += 1;
        if used_ids_contains_all(out, exp.must_include) {
            hits += 1;
        }
    }
    if !exp.must_exclude.is_empty() {
        total += 1;
        if used_ids_contains_none(out, exp.must_exclude) {
            hits += 1;
        }
    }
    Some(hits as f32 / total as f32)
}

pub fn provenance(out: &RecallResult, exp: &Expected) -> Option<f32> {
    if !exp.requires_citation {
        return None;
    }
    Some(if out.citations.is_empty() { 0.0 } else { 1.0 })
}

pub fn bitemporal_recall(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let causal = exp.required_warnings.contains(&"causal_mask_applied");
    let has_temporal = causal
        || !exp.must_exclude.is_empty()
        || (!exp.must_include.is_empty() && !exp.required_warnings.is_empty());
    if !has_temporal {
        return None;
    }
    let mut hits = 0u32;
    let mut total = 0u32;
    if !exp.must_include.is_empty() {
        total += 1;
        if used_ids_contains_all(out, exp.must_include) {
            hits += 1;
        }
    }
    if !exp.must_exclude.is_empty() {
        total += 1;
        if used_ids_contains_none(out, exp.must_exclude) {
            hits += 1;
        }
    }
    if causal {
        total += 1;
        if out
            .warnings
            .iter()
            .any(|w| w.name() == "causal_mask_applied")
        {
            hits += 1;
        }
    }
    if total == 0 {
        None
    } else {
        Some(hits as f32 / total as f32)
    }
}

pub fn contradiction(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let needed: Vec<&str> = exp
        .required_warnings
        .iter()
        .copied()
        .filter(|w| matches!(*w, "contradicted" | "stale" | "skeptic_surfaced"))
        .collect();
    if needed.is_empty() {
        return None;
    }
    let hits = needed
        .iter()
        .filter(|w| out.warnings.iter().any(|x| x.name() == **w))
        .count();
    Some(hits as f32 / needed.len() as f32)
}

pub fn math_science(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let has_unit = exp.required_warnings.contains(&"unit_mismatch");
    let science_terms: Vec<&str> = exp
        .must_contain
        .iter()
        .copied()
        .filter(|s| {
            let s = s.to_lowercase();
            s.contains("kg")
                || s.contains("gev")
                || s.contains("c")
                || s.contains("h_")
                || s.contains("f =")
                || s.contains("e =")
                || s.contains("c²")
                || s.contains("ev")
                || s.contains("σ")
        })
        .collect();
    if !has_unit && science_terms.is_empty() {
        return None;
    }
    let mut hits = 0u32;
    let mut total = 0u32;
    if has_unit {
        total += 1;
        if out.warnings.iter().any(|w| w.name() == "unit_mismatch") {
            hits += 1;
        }
    }
    if !science_terms.is_empty() {
        total += 1;
        if science_terms
            .iter()
            .all(|s| out.answer.to_lowercase().contains(&s.to_lowercase()))
        {
            hits += 1;
        }
    }
    Some(hits as f32 / total as f32)
}

pub fn english_discourse(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let english_targets: Vec<&str> = exp
        .must_contain
        .iter()
        .copied()
        .filter(|s| {
            let l = s.to_lowercase();
            l == "bob"
                || l == "alice"
                || l == "she"
                || l == "riverbank"
                || l == "stepped down"
                || l == "director"
        })
        .collect();
    if english_targets.is_empty() {
        return None;
    }
    let ok = answer_contains_all(out, &english_targets);
    Some(if ok { 1.0 } else { 0.0 })
}

pub fn privacy_redaction(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let canaries: Vec<&str> = exp
        .must_not_contain
        .iter()
        .copied()
        .filter(|s| {
            s.contains("sk-memory-bench-CANARY")
                || s.contains("AKIA")
                || s.contains("MEMORY_BENCH_CANARY")
                || s.contains("1985-03-12")
                || s.contains("memory-benchmark-test-corp")
                || s.contains("94110")
        })
        .collect();
    let needs_redacted_warning = exp.required_warnings.contains(&"redacted");
    if canaries.is_empty() && !needs_redacted_warning {
        return None;
    }
    let mut hits = 0u32;
    let mut total = 0u32;
    if !canaries.is_empty() {
        total += 1;
        if answer_contains_none(out, &canaries) {
            hits += 1;
        }
    }
    if needs_redacted_warning {
        total += 1;
        if out.warnings.iter().any(|w| w.name() == "redacted") {
            hits += 1;
        }
    }
    Some(hits as f32 / total as f32)
}

pub fn procedural_skill(out: &RecallResult, exp: &Expected) -> Option<f32> {
    // Heuristic: procedural fixtures tend to require_citation AND mention a
    // skill name in must_contain.
    let mentions_skill = exp.must_contain.iter().any(|s| {
        s.contains("normalize")
            || s.contains("UNSAFE")
            || s.contains("doi_")
            || s.contains("refuse")
    }) || exp
        .must_not_contain
        .iter()
        .any(|s| s.contains("fs_delete") || s.contains("net_exfil"));
    if !mentions_skill {
        return None;
    }
    let mut hits = 0u32;
    let mut total = 0u32;
    if !exp.must_contain.is_empty() {
        total += 1;
        if answer_contains_all(out, exp.must_contain) {
            hits += 1;
        }
    }
    if !exp.must_not_contain.is_empty() {
        total += 1;
        if answer_contains_none(out, exp.must_not_contain) {
            hits += 1;
        }
    }
    if total == 0 {
        None
    } else {
        Some(hits as f32 / total as f32)
    }
}

pub fn feedback_adaptation(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let (lo, hi) = exp.confidence_range?;
    if out.confidence >= lo && out.confidence <= hi {
        Some(1.0)
    } else {
        Some(0.0)
    }
}

pub fn determinism_rebuild(out: &RecallResult, exp: &Expected) -> Option<f32> {
    if !exp.expects_stable_state_hash {
        return None;
    }
    Some(if out.context_pack_hash.is_empty() {
        0.0
    } else {
        1.0
    })
}

/// New compounding axis — exercised only by the dedicated compounding
/// suite. Legacy T0 / T1 fixtures return `None` so the calibration band
/// on the existing 10 axes is preserved.
pub fn compounding(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let active = exp
        .required_warnings
        .iter()
        .any(|w| matches!(*w, "compound_chain" | "compound_follow_up" | "multi_hop"))
        || exp.must_contain.iter().any(|s| {
            let s = s.to_lowercase();
            s.contains("follow-up")
                || s.contains("applies")
                || s.contains("compound")
                || s.contains("distillation")
                || s.contains("transfer")
        })
        || exp.must_include.len() > 1
        || exp.must_exclude.len() > 1;
    if !active {
        return None;
    }

    let mut hits = 0u32;
    let mut total = 0u32;

    if !exp.must_include.is_empty() {
        total += 1;
        if used_ids_contains_all(out, exp.must_include) {
            hits += 1;
        }
    }
    if !exp.must_exclude.is_empty() {
        total += 1;
        if used_ids_contains_none(out, exp.must_exclude) {
            hits += 1;
        }
    }
    if !exp.must_contain.is_empty() {
        total += 1;
        if answer_contains_all(out, exp.must_contain) {
            hits += 1;
        }
    }
    if !exp.must_not_contain.is_empty() {
        total += 1;
        if answer_contains_none(out, exp.must_not_contain) {
            hits += 1;
        }
    }
    if exp.requires_citation {
        total += 1;
        if !out.citations.is_empty() {
            hits += 1;
        }
    }
    if out.confidence > 0.0 {
        total += 1;
        if out.confidence >= 0.5 {
            hits += 1;
        }
    }

    Some(if total == 0 {
        0.0
    } else {
        hits as f32 / total as f32
    })
}

/// New topic-hardening axis — exercised only by the dedicated hardening
/// suite. Legacy fixtures return `None`.
pub fn topic_hardening(out: &RecallResult, exp: &Expected) -> Option<f32> {
    let active = exp
        .required_warnings
        .iter()
        .any(|w| matches!(*w, "topic_hardened" | "repeat_recall" | "reinforced"))
        || exp
            .must_contain
            .iter()
            .any(|s| s.to_lowercase().contains("repeat"))
        || exp.confidence_range.is_some();
    if !active {
        return None;
    }

    let mut hits = 0u32;
    let mut total = 0u32;

    if exp.requires_citation {
        total += 1;
        if !out.citations.is_empty() {
            hits += 1;
        }
    }
    if let Some((lo, hi)) = exp.confidence_range {
        total += 1;
        if out.confidence >= lo && out.confidence <= hi {
            hits += 1;
        }
    }
    if !exp.must_contain.is_empty() {
        total += 1;
        if answer_contains_all(out, exp.must_contain) {
            hits += 1;
        }
    }
    if !exp.must_not_contain.is_empty() {
        total += 1;
        if answer_contains_none(out, exp.must_not_contain) {
            hits += 1;
        }
    }
    if out.context_token_count > 0 {
        total += 1;
        if out.context_token_count <= 256 {
            hits += 1;
        }
    }
    if !out.context_pack_hash.is_empty() {
        total += 1;
        if out.context_pack_hash.len() >= 8 {
            hits += 1;
        }
    }

    Some(if total == 0 {
        0.0
    } else {
        hits as f32 / total as f32
    })
}

// ───────── compatibility shim: grade_all_axes returns AxisScores ─────────
//
// Fixtures still reference `grade: scorer::grade_all_axes`. We compute each
// axis as Option<f32> internally, but expose a flat AxisScores struct where
// unexercised axes are encoded as f32::NAN. bench.rs's averaging strips NAN.

pub fn grade_all_axes(out: &RecallResult, exp: &Expected) -> AxisScores {
    AxisScores {
        correctness: correctness(out, exp).unwrap_or(f32::NAN),
        provenance: provenance(out, exp).unwrap_or(f32::NAN),
        bitemporal_recall: bitemporal_recall(out, exp).unwrap_or(f32::NAN),
        contradiction: contradiction(out, exp).unwrap_or(f32::NAN),
        math_science: math_science(out, exp).unwrap_or(f32::NAN),
        english_discourse_coreference: english_discourse(out, exp).unwrap_or(f32::NAN),
        privacy_redaction: privacy_redaction(out, exp).unwrap_or(f32::NAN),
        procedural_skill: procedural_skill(out, exp).unwrap_or(f32::NAN),
        feedback_adaptation: feedback_adaptation(out, exp).unwrap_or(f32::NAN),
        determinism_rebuild: determinism_rebuild(out, exp).unwrap_or(f32::NAN),
        compounding: compounding(out, exp).unwrap_or(f32::NAN),
        topic_hardening: topic_hardening(out, exp).unwrap_or(f32::NAN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecallResult;

    fn empty_recall() -> RecallResult {
        RecallResult {
            context_pack_hash: "deadbeefdeadbeef".to_string(),
            ..RecallResult::default()
        }
    }

    fn empty_expected() -> Expected {
        Expected {
            must_include: &[],
            must_exclude: &[],
            must_contain: &[],
            must_not_contain: &[],
            required_warnings: &[],
            requires_citation: false,
            expected_modality: None,
            confidence_range: None,
            expects_stable_state_hash: false,
        }
    }

    #[test]
    fn empty_fixture_returns_none_for_all_axes() {
        let r = empty_recall();
        let e = empty_expected();
        let a = grade_all_axes(&r, &e);
        // All NaN — unexercised.
        assert!(a.correctness.is_nan());
        assert!(a.provenance.is_nan());
        assert!(a.bitemporal_recall.is_nan());
    }

    #[test]
    fn provenance_axis_active_when_required() {
        let mut e = empty_expected();
        e.requires_citation = true;
        let r = empty_recall();
        assert_eq!(provenance(&r, &e), Some(0.0));
    }

    #[test]
    fn compounding_axis_is_inactive_without_markers() {
        let r = empty_recall();
        let e = empty_expected();
        assert_eq!(compounding(&r, &e), None);
    }

    #[test]
    fn compounding_axis_scores_when_marked() {
        let mut r = empty_recall();
        r.answer = "follow-up applies the same compound distillation".to_string();
        r.used_ids = vec!["a".to_string(), "b".to_string()];
        r.citations.push(crate::result::Citation {
            source_uri: "urn:test".to_string(),
            citation: "test".to_string(),
            quote: None,
        });
        let mut e = empty_expected();
        e.must_contain = &["follow-up"];
        e.must_include = &["a", "b"];
        e.requires_citation = true;
        e.required_warnings = &["compound_chain"];
        assert_eq!(compounding(&r, &e), Some(1.0));
    }

    #[test]
    fn topic_hardening_axis_is_inactive_without_markers() {
        let r = empty_recall();
        let e = empty_expected();
        assert_eq!(topic_hardening(&r, &e), None);
    }

    #[test]
    fn topic_hardening_axis_scores_when_marked() {
        let mut r = empty_recall();
        r.answer = "repeat recall keeps the topic reinforced".to_string();
        r.confidence = 0.75;
        r.context_token_count = 128;
        let mut e = empty_expected();
        e.must_contain = &["repeat"];
        e.requires_citation = true;
        e.confidence_range = Some((0.5, 0.8));
        e.required_warnings = &["topic_hardened"];
        r.citations.push(crate::result::Citation {
            source_uri: "urn:test".to_string(),
            citation: "test".to_string(),
            quote: None,
        });
        assert_eq!(topic_hardening(&r, &e), Some(1.0));
    }
}
