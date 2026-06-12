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
