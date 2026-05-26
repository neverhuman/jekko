//! Deterministic Hero/Judge scoring, artifact, and parser helpers.

use anyhow::Result;
use jekko_store::db::Db;

use crate::daemon_store;
use crate::hashing::{sha256_hex, sha256_json};
use crate::hero_judge::{
    HeroJudgeConfig, HeroJudgeLaneArtifact, HeroJudgeRunbook, KnowledgeEntry, PromotionDecision,
    PromptVariant,
};
use crate::model_client::CredentialSourcePolicy;
use crate::reasoning::{
    stable_reasoning_hash, AdvancedReasoningConfig, EvidenceLevel, MemoryCapsule,
    ReasoningArtifact, ReasoningArtifactKind, ReasoningRole,
};

pub use crate::hero_judge_eval_io::{
    write_json_pretty, write_jsonl, write_lane_metrics_csv, write_quality_csv,
    write_series_summary_csv,
};
pub use crate::hero_judge_eval_metrics::{
    average_score, generation_quality_metric, lane_metric_records, lane_quality_metrics,
    parse_substitute_lane_value, quality_trend,
};

// Re-export the public surface the rest of the crate already uses.
pub use crate::hero_judge_eval_metrics::{
    review_cards, reviewer_questions, role_group, rounded, score_from_value,
    scoreboard_for_generation, summary_from_value, synthetic_lane_value, GenerationMetricInputs,
};

pub(crate) fn reduce_generation(
    run_id: &str,
    generation: usize,
    heroes: &[HeroJudgeLaneArtifact],
    verifier_score: f64,
    red_team: &[HeroJudgeLaneArtifact],
    config: &HeroJudgeConfig,
) -> PromotionDecision {
    let red_team_penalty = red_team_penalty(red_team);
    let mut best: Option<(&HeroJudgeLaneArtifact, f64, String)> = None;
    for hero in heroes {
        let leak = leak_status(hero);
        let mut score =
            (hero.score * 0.70 + verifier_score * 0.25 - red_team_penalty).clamp(0.0, 1.0);
        if config.promotion.canary_replay && leak != "clean" {
            score = 0.0;
        }
        if config.promotion.anti_leak && leak != "clean" {
            score = 0.0;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_score, _)| score > *best_score)
        {
            best = Some((hero, score, leak));
        }
    }
    let Some((winner, score, leak)) = best else {
        return PromotionDecision {
            run_id: run_id.to_string(),
            generation,
            winner_candidate_id: None,
            winner_prompt_id: None,
            score: 0.0,
            promoted: false,
            reason: "no hero candidates".to_string(),
        };
    };
    let promoted = score >= config.promotion.min_score && leak == "clean";
    PromotionDecision {
        run_id: run_id.to_string(),
        generation,
        winner_candidate_id: Some(winner.id.clone()),
        winner_prompt_id: Some(format!("prompt-{}", winner.id)),
        score,
        promoted,
        reason: if promoted {
            "passed deterministic host score, canary replay, and anti-leak gates".to_string()
        } else if leak != "clean" {
            format!("rejected by anti-leak gate: {leak}")
        } else {
            format!(
                "score {:.3} below promotion gate {:.3}",
                score, config.promotion.min_score
            )
        },
    }
}

pub(crate) fn knowledge_entry(
    generation: usize,
    decision: &PromotionDecision,
    evidence: &[crate::evidence::LoadedEvidence],
) -> KnowledgeEntry {
    let status = if decision.promoted {
        "verified"
    } else {
        "rejected"
    };
    let claim = if decision.promoted {
        format!(
            "Generation {generation} prompt variant {} passed the deterministic OpenQG promotion gates.",
            decision
                .winner_prompt_id
                .as_deref()
                .unwrap_or("unknown-prompt")
        )
    } else {
        format!(
            "Generation {generation} prompt variant was not promoted: {}.",
            decision.reason
        )
    };
    let mut entry = KnowledgeEntry {
        id: format!("knowledge-g{generation:03}"),
        generation,
        status: status.to_string(),
        claim,
        evidence_refs: evidence.iter().map(|item| item.id.clone()).collect(),
        source_candidate_id: decision.winner_candidate_id.clone(),
        content_sha256: String::new(),
    };
    entry.content_sha256 = sha256_json(&entry, "knowledge_entry");
    entry
}

pub(crate) fn persist_knowledge_capsule(
    db: &Db,
    run_id: &str,
    entry: &KnowledgeEntry,
) -> Result<()> {
    let config = AdvancedReasoningConfig::default();
    let mut artifact = ReasoningArtifact::new(
        format!("artifact-{}", entry.id),
        run_id,
        ReasoningRole::MemoryCurator,
        ReasoningArtifactKind::MemoryCapsule,
        format!("Hero/Judge {}", entry.id),
        entry.claim.clone(),
        EvidenceLevel::ExternalGrounding,
        if entry.status == "verified" { 0.8 } else { 0.6 },
        serde_json::to_value(entry)?,
    );
    artifact.prepare_for_storage(&config);
    daemon_store::persist_reasoning_artifact(db, run_id, &artifact)?;
    let memory = MemoryCapsule {
        id: entry.id.clone(),
        run_id: run_id.to_string(),
        artifact_id: artifact.id,
        scope: "openqg".to_string(),
        status: entry.status.clone(),
        summary: entry.claim.clone(),
        evidence_level: EvidenceLevel::ExternalGrounding,
        confidence: if entry.status == "verified" { 0.8 } else { 0.6 },
        payload_json: serde_json::to_value(entry)?,
        memory_kind: zyal_core::MemoryKind::Semantic,
        promotion_status: zyal_core::MemoryPromotionStatus::Scratch,
        claim_text: entry.claim.clone(),
        approved_by_role: None,
        content_hash: stable_reasoning_hash(entry),
    };
    daemon_store::persist_memory_capsule(db, run_id, &memory)?;
    Ok(())
}

pub(crate) fn seed_prompt_lineage(objective: &str, config: &HeroJudgeConfig) -> Vec<PromptVariant> {
    let hero_seed = format!("hero seed: {objective}");
    let judge_seed = format!("judge seed: {objective}");
    vec![
        PromptVariant {
            id: "hero-seed".to_string(),
            role: "hero".to_string(),
            generation: 0,
            parent_id: None,
            summary: "Seed hero prompt for OpenQG theory candidate generation.".to_string(),
            prompt_sha256: sha256_hex(hero_seed.as_bytes()),
            score: 0.0,
            status: "seed".to_string(),
        },
        PromptVariant {
            id: "judge-seed".to_string(),
            role: "judge".to_string(),
            generation: 0,
            parent_id: None,
            summary: format!(
                "Seed judge prompt with promotion threshold {:.3}.",
                config.promotion.min_score
            ),
            prompt_sha256: sha256_hex(judge_seed.as_bytes()),
            score: 0.0,
            status: "seed".to_string(),
        },
    ]
}

pub(crate) fn prompt_for(
    role: &str,
    objective: &str,
    generation: usize,
    evidence: &[crate::evidence::LoadedEvidence],
    receipts: &[crate::hero_judge::HeroJudgeSearchReceipt],
) -> String {
    let evidence_refs = evidence
        .iter()
        .map(|item| format!("{}:{}:{}", item.id, item.role, item.sha256))
        .collect::<Vec<_>>()
        .join(", ");
    let research_refs = receipts
        .iter()
        .map(|receipt| format!("{}:{}:{}", receipt.id, receipt.status, receipt.url_count))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Role: {role}. Objective: {objective}. Generation: {generation}. Evidence: [{evidence_refs}]. Research receipts: [{research_refs}]. Return only compact JSON with summary:string, claims:string[], questions:string[], rubric:string[], evidence_refs:string[], score:number. Do not return private reasoning. Hero lanes must submit artifact-first theory prospects: formal objects, assumptions, derivation gaps, falsifier questions, constants ledger, extraction map, and unsupported rows as compact claims/questions. Judge, verifier, red-team, and meta lanes must improve rubric calibration, leakage detection, hidden-parameter checks, prior-art discipline, extraction validity, and reviewer questions. Reward honest structural progress; do not over-score ideas that only repackage known theory or quote observed constants."
    )
}

pub(crate) fn run_objective(runbook: &HeroJudgeRunbook, config: &HeroJudgeConfig) -> String {
    match config
        .objective
        .clone()
        .or_else(|| runbook.job.as_ref().map(|job| job.objective.clone()))
    {
        Some(objective) => objective,
        None => "Evolve OpenQG hero and judge prompts".to_string(),
    }
}

pub(crate) fn validate_config(config: &HeroJudgeConfig) -> Result<()> {
    if config.generations == 0 {
        anyhow::bail!("hero_judge.generations must be at least 1");
    }
    if config.population.hero_lanes == 0 {
        anyhow::bail!("hero_judge.population.hero_lanes must be at least 1");
    }
    if config.population.judge_lanes == 0 {
        anyhow::bail!("hero_judge.population.judge_lanes must be at least 1");
    }
    if config.budgets.model_calls == 0 {
        anyhow::bail!("hero_judge.budgets.model_calls must be at least 1");
    }
    if config.population.max_parallel > crate::superreasoning::MAX_SUPERREASONING_WORKERS {
        anyhow::bail!("hero_judge.population.max_parallel must be <= 10");
    }
    if !config.super_reasoning.enabled {
        anyhow::bail!("hero_judge.super_reasoning.enabled must be true");
    }
    if config.super_reasoning.max_workers > crate::superreasoning::MAX_SUPERREASONING_WORKERS {
        anyhow::bail!("hero_judge.super_reasoning.max_workers must be <= 10");
    }
    if config.super_reasoning.credential_policy != CredentialSourcePolicy::UsersOnly {
        anyhow::bail!("hero_judge.super_reasoning.credential_policy must be users-only");
    }
    if !config.promotion.min_score.is_finite() {
        anyhow::bail!("hero_judge.promotion.min_score must be finite");
    }
    Ok(())
}

pub(crate) fn zyal_yaml_body(text: &str) -> Result<String> {
    let lines = text.lines().collect::<Vec<_>>();
    let Some((sentinel_idx, first)) = lines.iter().enumerate().find(|(_, line)| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    }) else {
        anyhow::bail!("empty ZYAL document");
    };
    if !first.starts_with("<<<ZYAL ") {
        return Ok(text.to_string());
    }
    let mut body = Vec::new();
    for line in lines.into_iter().skip(sentinel_idx + 1) {
        if line.starts_with("<<<END_ZYAL ") {
            return Ok(body.join("\n"));
        }
        body.push(line);
    }
    anyhow::bail!("missing END_ZYAL sentinel")
}

fn red_team_penalty(artifacts: &[HeroJudgeLaneArtifact]) -> f64 {
    if artifacts.is_empty() {
        return 0.0;
    }
    let pressure = artifacts
        .iter()
        .map(|artifact| metric_value(artifact, "red_team_pressure", artifact.score * 0.50))
        .sum::<f64>()
        / artifacts.len() as f64;
    (pressure * 0.08).clamp(0.0, 0.08)
}

fn leak_status(artifact: &HeroJudgeLaneArtifact) -> String {
    let text = artifact.summary.to_ascii_lowercase();
    if text.contains("hidden_canary")
        || text.contains("fixture_leak")
        || text.contains("hidden constant")
        || text.contains("raw_chain_of_thought")
        || text.contains("chain_of_thought")
    {
        "leak_detected".to_string()
    } else {
        "clean".to_string()
    }
}

fn metric_value(artifact: &HeroJudgeLaneArtifact, key: &str, default_score: f64) -> f64 {
    artifact
        .metrics
        .get(key)
        .copied()
        .unwrap_or(default_score)
        .clamp(0.0, 1.0)
}
