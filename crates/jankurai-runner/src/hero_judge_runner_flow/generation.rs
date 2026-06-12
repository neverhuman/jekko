mod prompted;
mod types;

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;

use crate::daemon_store;
use crate::events::EventKind;
use crate::hero_judge::{
    HeroJudgeConfig, HeroJudgeLaneMetric, HeroJudgeQualityMetric, PromotionDecision, PromptVariant,
};
use crate::hero_judge_eval::{
    average_score, generation_quality_metric, knowledge_entry, lane_metric_records,
    persist_knowledge_capsule, reduce_generation, review_cards, rounded, scoreboard_for_generation,
    seed_prompt_lineage, write_json_pretty, write_jsonl, GenerationMetricInputs, OracleVerdict,
};
use crate::hero_judge_runner_helpers::evolution_context;
use crate::model_client::kind_label;
use crate::model_policy::ModelTaskKind;

use self::prompted::run_prompted_group;
pub(super) use self::types::{GenerationInputs, GenerationState};

/// One rejected dead-end approach, accumulated across generations so later
/// generations neither re-walk it (prompt feed-forward) nor re-promote it
/// (exact-sha dedup in `reduce_generation`).
#[derive(Debug, Clone, Serialize)]
struct NegativeApproach {
    generation: usize,
    content_sha256: String,
    summary: String,
    reason: String,
}

/// Append the accumulated negative memory to a generation's evolution context
/// as explicit "do not repeat" constraints. No-op when empty (behavior
/// unchanged for the first generation / runs that never reject).
fn append_negative_context(mut context: String, negatives: &[NegativeApproach]) -> String {
    if negatives.is_empty() {
        return context;
    }
    context.push_str("\nPreviously rejected approaches — do NOT repeat these dead ends: ");
    for (idx, entry) in negatives.iter().rev().take(8).enumerate() {
        if idx > 0 {
            context.push_str("; ");
        }
        let summary: String = entry.summary.chars().take(160).collect();
        context.push_str(&format!(
            "[gen {} rejected ({})] {}",
            entry.generation, entry.reason, summary
        ));
    }
    context.push_str(". Propose a materially different approach.");
    context
}

/// Run the operator-supplied objective oracle command (if any) in the run repo
/// and convert its exit status into an [`OracleVerdict`]. Returns `Ok(None)`
/// when no command is configured, preserving the self-graded behavior. The
/// command comes from trusted run config (`promotion.objective_command`), not
/// model output, so it is equivalent to a CI check.
fn compute_objective_oracle(
    repo: &Path,
    config: &HeroJudgeConfig,
) -> Result<Option<OracleVerdict>> {
    let Some(command) = config.promotion.objective_command.as_deref() else {
        return Ok(None);
    };
    if command.trim().is_empty() {
        return Ok(None);
    }
    // Command floor: refuse to spawn a catastrophic objective oracle command.
    if let Some(reason) = crate::command_floor::blocked_reason(command) {
        anyhow::bail!("command floor blocked objective oracle command ({reason}): {command}");
    }
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(repo)
        .status()
        .with_context(|| format!("run objective oracle command `{command}`"))?;
    let passed = status.success();
    Ok(Some(OracleVerdict {
        passed,
        score: if passed { 1.0 } else { 0.0 },
    }))
}

pub(super) async fn run_generations(input: GenerationInputs<'_>) -> Result<GenerationState> {
    let mut prompt_lineage = seed_prompt_lineage(input.objective, input.config);
    let mut scoreboard = Vec::new();
    let mut knowledge = Vec::new();
    let mut quality_metrics: Vec<HeroJudgeQualityMetric> = Vec::new();
    let mut lane_metrics: Vec<HeroJudgeLaneMetric> = Vec::new();
    let mut reviewer_cards = Vec::new();
    let mut model_calls_used = 0_usize;
    let mut last_model_kind = None;
    let mut last_decision = PromotionDecision {
        run_id: input.run_id.to_string(),
        generation: 0,
        winner_candidate_id: None,
        winner_prompt_id: None,
        score: 0.0,
        promoted: false,
        reason: "no generation completed".to_string(),
    };
    let mut frontier_parent = Some("hero-seed".to_string());
    // Negative memory accumulated across generations (U3): rejected dead ends
    // feed forward into prompts and block exact re-promotion.
    let mut negative_memory: Vec<NegativeApproach> = Vec::new();
    let mut rejected_shas: HashSet<String> = HashSet::new();

    for generation in 1..=input.generations {
        daemon_store::mark_daemon_run(
            input.db,
            input.run_id,
            "running",
            &format!("hero_judge_generation_{generation}"),
            None,
        )?;
        input.sink.emit(
            EventKind::HeroJudgeGeneration,
            json!({"generation": generation}),
        )?;
        let gen_dir = input.output_dir.join(format!("generation-{generation:03}"));
        fs::create_dir_all(&gen_dir).with_context(|| format!("mkdir {}", gen_dir.display()))?;
        let evolution_context =
            evolution_context(generation, &last_decision, quality_metrics.last());
        let evolution_context = append_negative_context(evolution_context, &negative_memory);

        let literature = run_prompted_group(
            &input,
            generation,
            "literature synthesis",
            ModelTaskKind::LiteratureSynthesis,
            input.config.population.literature_lanes,
            &evolution_context,
        )
        .await?;
        model_calls_used += literature.len();
        write_json_pretty(&gen_dir.join("literature.json"), &literature)?;

        let heroes = run_prompted_group(
            &input,
            generation,
            "hero candidate",
            ModelTaskKind::HeroGenerate,
            input.config.population.hero_lanes,
            &evolution_context,
        )
        .await?;
        model_calls_used += heroes.len();
        for hero in &heroes {
            input.sink.emit(
                EventKind::HeroCandidate,
                json!({"id": hero.id, "generation": generation, "score": rounded(hero.score)}),
            )?;
            prompt_lineage.push(PromptVariant {
                id: format!("prompt-{}", hero.id),
                role: "hero".to_string(),
                generation,
                parent_id: frontier_parent.clone(),
                summary: hero.summary.clone(),
                prompt_sha256: hero.content_sha256.clone(),
                score: hero.score,
                status: "candidate".to_string(),
            });
        }
        write_json_pretty(&gen_dir.join("hero-candidates.json"), &heroes)?;

        let judges = run_prompted_group(
            &input,
            generation,
            "judge patch",
            ModelTaskKind::JudgePatch,
            input.config.population.judge_lanes,
            &evolution_context,
        )
        .await?;
        model_calls_used += judges.len();
        for judge in &judges {
            input.sink.emit(
                EventKind::JudgePatch,
                json!({"id": judge.id, "generation": generation}),
            )?;
            prompt_lineage.push(PromptVariant {
                id: format!("prompt-{}", judge.id),
                role: "judge".to_string(),
                generation,
                parent_id: Some("judge-seed".to_string()),
                summary: judge.summary.clone(),
                prompt_sha256: judge.content_sha256.clone(),
                score: judge.score,
                status: "candidate".to_string(),
            });
        }
        write_json_pretty(&gen_dir.join("judge-patches.json"), &judges)?;

        let verifiers = run_prompted_group(
            &input,
            generation,
            "verifier",
            ModelTaskKind::Verifier,
            input.config.population.verifier_lanes,
            &evolution_context,
        )
        .await?;
        model_calls_used += verifiers.len();
        let verifier_score = average_score(&verifiers, 0.84);
        input.sink.emit(
            EventKind::VerifierScore,
            json!({"generation": generation, "score": rounded(verifier_score)}),
        )?;
        write_json_pretty(&gen_dir.join("verifier-scores.json"), &verifiers)?;

        let red_team = run_prompted_group(
            &input,
            generation,
            "red team",
            ModelTaskKind::RedTeam,
            input.config.population.red_team_lanes,
            &evolution_context,
        )
        .await?;
        model_calls_used += red_team.len();
        write_json_pretty(&gen_dir.join("red-team.json"), &red_team)?;

        let meta = run_prompted_group(
            &input,
            generation,
            "meta judge reducer",
            ModelTaskKind::MetaJudge,
            1,
            &evolution_context,
        )
        .await?;
        model_calls_used += meta.len();
        write_json_pretty(&gen_dir.join("meta-judge.json"), &meta)?;

        // Ground the promotion in an objective oracle when configured: a failing
        // oracle hard-rejects regardless of model self-scores.
        let oracle = compute_objective_oracle(input.repo, input.config)?;
        if let Some(verdict) = oracle.as_ref() {
            write_json_pretty(&gen_dir.join("objective-oracle.json"), verdict)?;
        }
        let decision = reduce_generation(
            input.run_id,
            generation,
            &heroes,
            verifier_score,
            &red_team,
            input.config,
            oracle,
            &rejected_shas,
        );
        input.sink.emit(
            EventKind::PromotionDecision,
            json!({"generation": generation, "promoted": decision.promoted, "score": rounded(decision.score)}),
        )?;
        if let Some(winner) = decision.winner_candidate_id.as_deref() {
            frontier_parent = Some(format!("prompt-{winner}"));
        }
        // Record the best-but-rejected approach as negative memory so later
        // generations avoid it (prompt feed-forward) and cannot re-promote it.
        if !decision.promoted {
            if let Some(winner) = decision
                .winner_candidate_id
                .as_deref()
                .and_then(|id| heroes.iter().find(|hero| hero.id == id))
            {
                if rejected_shas.insert(winner.content_sha256.clone()) {
                    negative_memory.push(NegativeApproach {
                        generation,
                        content_sha256: winner.content_sha256.clone(),
                        summary: winner.summary.clone(),
                        reason: decision.reason.clone(),
                    });
                    write_jsonl(
                        &input.output_dir.join("negative-memory.jsonl"),
                        &negative_memory,
                    )?;
                }
            }
        }
        scoreboard.extend(scoreboard_for_generation(
            generation,
            &heroes,
            verifier_score,
            &red_team,
            &decision,
        ));
        write_json_pretty(&gen_dir.join("promotion-decision.json"), &decision)?;
        last_decision = decision;

        let curated = run_prompted_group(
            &input,
            generation,
            "knowledge curator",
            ModelTaskKind::KnowledgeCurate,
            1,
            &evolution_context,
        )
        .await?;
        model_calls_used += curated.len();
        last_model_kind = Some(kind_label(ModelTaskKind::KnowledgeCurate).to_string());
        let entry = knowledge_entry(generation, &last_decision, input.evidence);
        persist_knowledge_capsule(input.db, input.run_id, &entry)?;
        input.sink.emit(
            EventKind::KnowledgeCompounded,
            json!({"id": entry.id, "status": entry.status}),
        )?;
        knowledge.push(entry);

        let previous_overall = quality_metrics
            .last()
            .map(|metric| metric.overall_quality_index);
        let previous_frontier = quality_metrics
            .last()
            .map(|metric| metric.frontier_quality_index);
        let quality_metric = generation_quality_metric(GenerationMetricInputs {
            run_id: input.run_id,
            generation,
            literature: &literature,
            heroes: &heroes,
            judges: &judges,
            verifiers: &verifiers,
            red_team: &red_team,
            meta: &meta,
            decision: &last_decision,
            search_receipts: input.search_receipts,
            previous_overall,
            previous_frontier,
            knowledge_entry_count: knowledge.len(),
        });
        input.sink.emit(
            EventKind::HeroJudgeGeneration,
            json!({
                "generation": generation,
                "overall_quality_index": quality_metric.overall_quality_index,
                "theory_quality_index": quality_metric.theory_quality_index,
                "question_quality_index": quality_metric.question_quality_index,
                "rubric_quality_index": quality_metric.rubric_quality_index,
                "delta_overall_quality": quality_metric.delta_overall_quality,
                "frontier_quality_index": quality_metric.frontier_quality_index,
                "delta_frontier_quality": quality_metric.delta_frontier_quality,
            }),
        )?;
        write_json_pretty(&gen_dir.join("quality-metrics.json"), &quality_metric)?;
        quality_metrics.push(quality_metric);
        let generation_lane_metrics = lane_metric_records(
            input.run_id,
            &[
                &literature,
                &heroes,
                &judges,
                &verifiers,
                &red_team,
                &meta,
                &curated,
            ],
        );
        write_jsonl(
            &gen_dir.join("lane-metrics.jsonl"),
            &generation_lane_metrics,
        )?;
        lane_metrics.extend(generation_lane_metrics);
        reviewer_cards.extend(review_cards(&[
            &literature,
            &heroes,
            &judges,
            &verifiers,
            &red_team,
            &meta,
            &curated,
        ]));
    }

    Ok(GenerationState {
        model_calls_used,
        last_model_kind,
        last_decision,
        prompt_lineage,
        scoreboard,
        knowledge,
        quality_metrics,
        lane_metrics,
        reviewer_cards,
    })
}
