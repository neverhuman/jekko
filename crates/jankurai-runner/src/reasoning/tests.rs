use serde_json::json;

use super::*;
use crate::port::MAX_PORT_WORKERS;

#[test]
fn worker_cap_is_clamped() {
    let config = AdvancedReasoningConfig {
        worker_cap: 99,
        ..AdvancedReasoningConfig::default()
    };
    assert_eq!(config.effective_worker_cap(), MAX_PORT_WORKERS);
}

#[test]
fn confidence_caps_without_executable_evidence() {
    let config = AdvancedReasoningConfig::default();
    let mut artifact = ReasoningArtifact::new(
        "a1",
        "run",
        ReasoningRole::Planner,
        ReasoningArtifactKind::StageProposal,
        "plan",
        "summary",
        EvidenceLevel::IndependentAgreement,
        0.92,
        json!({"claim": "x"}),
    );
    artifact.prepare_for_storage(&config);
    assert_eq!(artifact.confidence, DEFAULT_CONFIDENCE_CAP);

    artifact.evidence_level = EvidenceLevel::Executable;
    artifact.confidence = 0.92;
    artifact.prepare_for_storage(&config);
    assert_eq!(artifact.confidence, 0.92);
}

#[test]
fn raw_reasoning_is_redacted_by_default() {
    let config = AdvancedReasoningConfig::default();
    let mut artifact = ReasoningArtifact::new(
        "a1",
        "run",
        ReasoningRole::Planner,
        ReasoningArtifactKind::StageProposal,
        "plan",
        "summary",
        EvidenceLevel::Unsupported,
        0.5,
        json!({}),
    );
    artifact.raw_reasoning = Some("private reasoning".into());
    artifact.prepare_for_storage(&config);
    assert_eq!(artifact.raw_reasoning, None);
}

#[test]
fn stable_hash_is_repeatable() {
    let one = stable_reasoning_hash(&json!({"a": 1, "b": ["x"]}));
    let two = stable_reasoning_hash(&json!({"a": 1, "b": ["x"]}));
    assert_eq!(one, two);
    assert_eq!(one.len(), 64);
}

#[test]
fn edge_validation_rejects_self_edges() {
    let edge = ReasoningEdge {
        run_id: "run".into(),
        src_artifact_id: "a".into(),
        dst_artifact_id: "a".into(),
        kind: "supports".into(),
        weight: Some(1.0),
        payload_json: json!({}),
    };
    assert!(edge.validate().is_err());
}

#[test]
fn permanent_memory_requires_verified_or_rejected_evidence() {
    let capsule = MemoryCapsule {
        id: "m1".into(),
        run_id: "run".into(),
        artifact_id: "a1".into(),
        scope: "repo".into(),
        status: "verified".into(),
        summary: "lesson".into(),
        evidence_level: EvidenceLevel::ExternalGrounding,
        confidence: 0.8,
        payload_json: json!({}),
        content_hash: "hash".into(),
        memory_kind: zyal_core::MemoryKind::Semantic,
        promotion_status: zyal_core::MemoryPromotionStatus::Scratch,
        claim_text: String::new(),
        approved_by_role: None,
    };
    assert!(capsule.can_write_permanent());
}
