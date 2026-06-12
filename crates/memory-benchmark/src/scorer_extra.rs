use crate::fixture::Expected;
use crate::RecallResult;

use super::{
    answer_contains_all, answer_contains_none, used_ids_contains_all, used_ids_contains_none,
};

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
