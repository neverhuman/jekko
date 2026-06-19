use crate::hero_judge::{
    FrontierScore, HeroJudgeLaneArtifact, HeroJudgeReviewCard, PromotionDecision,
};

use super::defaults::rounded;
use super::helpers::{leak_status, red_team_penalty, storage_safe_summary};
use super::lane::role_group;

pub fn scoreboard_for_generation(
    generation: usize,
    heroes: &[HeroJudgeLaneArtifact],
    verifier_score: f64,
    red_team: &[HeroJudgeLaneArtifact],
    decision: &PromotionDecision,
) -> Vec<FrontierScore> {
    let penalty = red_team_penalty(red_team);
    heroes
        .iter()
        .map(|hero| {
            let leak_status = leak_status(hero);
            let score = if leak_status == "clean" {
                (hero.score * 0.70 + verifier_score * 0.25 - penalty).clamp(0.0, 1.0)
            } else {
                0.0
            };
            FrontierScore {
                candidate_id: hero.id.clone(),
                prompt_id: format!("prompt-{}", hero.id),
                generation,
                score,
                verifier_score,
                red_team_penalty: penalty,
                leak_status,
                status: if decision.winner_candidate_id.as_deref() == Some(hero.id.as_str())
                    && decision.promoted
                {
                    "promoted".to_string()
                } else {
                    "scored".to_string()
                },
            }
        })
        .collect()
}

pub fn review_cards(groups: &[&[HeroJudgeLaneArtifact]]) -> Vec<HeroJudgeReviewCard> {
    groups
        .iter()
        .flat_map(|group| group.iter())
        .map(|artifact| HeroJudgeReviewCard {
            artifact_id: artifact.id.clone(),
            role_group: role_group(&artifact.kind).to_string(),
            kind: artifact.kind.clone(),
            generation: artifact.generation,
            lane: artifact.lane,
            score: rounded(artifact.score),
            summary: storage_safe_summary(&artifact.summary),
            content_sha256: artifact.content_sha256.clone(),
            metrics: artifact.metrics.clone(),
        })
        .collect()
}

pub fn reviewer_questions() -> Vec<String> {
    vec![
        "Are hero artifacts becoming more derivable, falsifiable, and evidence-grounded across generations?".to_string(),
        "Are judge artifacts becoming better calibrated, less gameable, and more explicit about leakage, hidden parameters, and extraction maps?".to_string(),
        "Does the retained frontier improve without storing private reasoning or importing fixture constants?".to_string(),
    ]
}
