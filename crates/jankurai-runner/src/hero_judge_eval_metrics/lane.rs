use std::collections::{BTreeMap, HashSet};

use crate::evidence::LoadedEvidence;
use crate::hero_judge::{HeroJudgeLaneArtifact, HeroJudgeLaneMetric, HeroJudgeSearchReceipt};
use crate::model_policy::ModelTaskKind;

use super::defaults::rounded;
use super::helpers::{
    arrayish_count, arrayish_strings, evidence_grounding_score, metric_value, normalized,
    storage_safety_score, structural_score,
};

pub fn lane_quality_metrics(
    kind: ModelTaskKind,
    value: &serde_json::Value,
    summary: &str,
    score: f64,
    known_keys: &HashSet<String>,
) -> BTreeMap<String, f64> {
    let claims = arrayish_count(value, &["claims", "hypotheses", "theories"]);
    let questions = arrayish_count(
        value,
        &["questions", "research_questions", "hard_questions"],
    );
    let rubric = arrayish_count(value, &["rubric", "criteria", "scoring_rubric"]);
    let evidence_ref_strings = arrayish_strings(value, &["evidence_refs", "citations", "sources"]);
    let evidence_refs = evidence_ref_strings.len();
    let evidence_grounding = evidence_grounding_score(value, &evidence_ref_strings, known_keys);
    let storage_safety = storage_safety_score(value, summary);
    let structural = structural_score(kind, value, claims, questions, rubric, evidence_refs);
    let claim_quality =
        (score * 0.55 + normalized(claims, 4) * 0.25 + evidence_grounding * 0.20).clamp(0.0, 1.0);
    let question_quality = if kind == ModelTaskKind::HeroGenerate {
        (score * 0.45
            + normalized(questions, 5) * 0.30
            + normalized(claims, 4) * 0.10
            + evidence_grounding * 0.15)
            .clamp(0.0, 1.0)
    } else {
        (score * 0.35 + normalized(questions, 3) * 0.25 + evidence_grounding * 0.15 + 0.25)
            .clamp(0.0, 1.0)
    };
    let rubric_quality = if matches!(
        kind,
        ModelTaskKind::JudgePatch
            | ModelTaskKind::Verifier
            | ModelTaskKind::MetaJudge
            | ModelTaskKind::RedTeam
    ) {
        (score * 0.40 + normalized(rubric, 5) * 0.35 + evidence_grounding * 0.15 + 0.10)
            .clamp(0.0, 1.0)
    } else {
        (score * 0.35 + normalized(rubric, 3) * 0.25 + evidence_grounding * 0.15 + 0.25)
            .clamp(0.0, 1.0)
    };

    let mut metrics = BTreeMap::from([
        ("claim_count".to_string(), claims as f64),
        ("claim_quality".to_string(), rounded(claim_quality)),
        (
            "evidence_grounding".to_string(),
            rounded(evidence_grounding),
        ),
        ("question_count".to_string(), questions as f64),
        ("question_quality".to_string(), rounded(question_quality)),
        ("rubric_item_count".to_string(), rubric as f64),
        ("rubric_quality".to_string(), rounded(rubric_quality)),
        ("storage_safety".to_string(), rounded(storage_safety)),
        ("structural_completeness".to_string(), rounded(structural)),
    ]);
    if kind == ModelTaskKind::RedTeam {
        let red_team_pressure = (score * 0.35
            + normalized(questions, 4) * 0.25
            + normalized(rubric, 4) * 0.20
            + (1.0 - storage_safety) * 0.20)
            .clamp(0.0, 1.0);
        metrics.insert("red_team_pressure".to_string(), rounded(red_team_pressure));
    }
    metrics
}

pub fn lane_metric_records(
    run_id: &str,
    groups: &[&[HeroJudgeLaneArtifact]],
) -> Vec<HeroJudgeLaneMetric> {
    groups
        .iter()
        .flat_map(|group| group.iter())
        .map(|artifact| HeroJudgeLaneMetric {
            run_id: run_id.to_string(),
            generation: artifact.generation,
            role_group: role_group(&artifact.kind).to_string(),
            kind: artifact.kind.clone(),
            artifact_id: artifact.id.clone(),
            lane: artifact.lane,
            score: rounded(artifact.score),
            claim_quality: rounded(metric_value(artifact, "claim_quality", artifact.score)),
            question_quality: rounded(metric_value(artifact, "question_quality", artifact.score)),
            rubric_quality: rounded(metric_value(artifact, "rubric_quality", artifact.score)),
            evidence_grounding: rounded(metric_value(artifact, "evidence_grounding", 0.0)),
            structural_completeness: rounded(metric_value(
                artifact,
                "structural_completeness",
                0.0,
            )),
            storage_safety: rounded(metric_value(artifact, "storage_safety", 1.0)),
            claim_count: metric_value(artifact, "claim_count", 0.0),
            question_count: metric_value(artifact, "question_count", 0.0),
            rubric_item_count: metric_value(artifact, "rubric_item_count", 0.0),
            model_receipt_id: artifact.model_receipt_id.clone(),
            content_sha256: artifact.content_sha256.clone(),
            status: artifact.status.clone(),
        })
        .collect()
}

/// Build the set of resolvable evidence keys for grounding: loaded-evidence
/// ids, sources, and hashes, plus the ids/hashes of research receipts that
/// actually fetched something (`url_count > 0`). Used by `run_lane_group` to
/// verify a candidate's claimed citations resolve to real evidence. An empty
/// result means "no evidence loaded", which the grounding scorer treats as the
/// offline fallback.
pub fn build_evidence_keys(
    evidence: &[LoadedEvidence],
    receipts: &[HeroJudgeSearchReceipt],
) -> HashSet<String> {
    let mut keys = HashSet::new();
    for item in evidence {
        keys.insert(item.id.to_ascii_lowercase());
        let source = item.source.trim();
        if !source.is_empty() {
            keys.insert(source.to_ascii_lowercase());
        }
        keys.insert(item.sha256.to_ascii_lowercase());
    }
    for receipt in receipts {
        if receipt.url_count > 0 {
            keys.insert(receipt.id.to_ascii_lowercase());
            keys.insert(receipt.content_sha256.to_ascii_lowercase());
        }
    }
    keys
}

pub fn role_group(kind: &str) -> &'static str {
    match kind {
        "hero_generate" => "hero",
        "judge_patch" | "verifier" | "red_team" | "meta_judge" => "judge",
        "literature_synthesis" => "research",
        "knowledge_curate" => "knowledge",
        _ => "other",
    }
}

#[cfg(test)]
mod grounding_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grounding_rewards_resolving_refs_over_fabricated() {
        let mut known = HashSet::new();
        known.insert("ev-1".to_string());
        let resolved = lane_quality_metrics(
            ModelTaskKind::HeroGenerate,
            &json!({"evidence_refs": ["ev-1"], "claims": ["x"]}),
            "summary",
            0.5,
            &known,
        );
        let fabricated = lane_quality_metrics(
            ModelTaskKind::HeroGenerate,
            &json!({"evidence_refs": ["totally-made-up"], "claims": ["x"]}),
            "summary",
            0.5,
            &known,
        );
        assert!(
            resolved["evidence_grounding"] > fabricated["evidence_grounding"],
            "a resolving citation should ground higher than a fabricated one: {} vs {}",
            resolved["evidence_grounding"],
            fabricated["evidence_grounding"],
        );
        assert_eq!(fabricated["evidence_grounding"], 0.0);
    }

    #[test]
    fn empty_known_keys_uses_offline_fallback() {
        // No loaded evidence: the prior marker heuristic still scores claims so
        // deterministic/offline runs are not regressed.
        let metrics = lane_quality_metrics(
            ModelTaskKind::HeroGenerate,
            &json!({"evidence_refs": ["a", "b"], "claims": ["x"]}),
            "summary",
            0.5,
            &HashSet::new(),
        );
        assert!(metrics["evidence_grounding"] > 0.0);
    }
}
