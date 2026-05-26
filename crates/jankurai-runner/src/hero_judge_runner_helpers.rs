//! Helpers extracted from [`hero_judge_runner_flow`]. These are pure
//! formatting/coordination utilities; the orchestrator stays focused on the
//! state machine and the helper file stays small.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use futures::future::join_all;
use jekko_store::db::Db;

use crate::events::EventSink;
use crate::hashing::{sha256_hex, sha256_json};
use crate::hero_judge::{
    HeroJudgeLaneArtifact, HeroJudgeLaneMetric, HeroJudgeQualityMetric, HeroJudgeRunbook,
    PromotionDecision,
};
use crate::hero_judge_eval::{lane_quality_metrics, score_from_value, summary_from_value};
use crate::hero_judge_runner_completion::{complete_hero_json, HeroJudgeCompletionContext};
use crate::model_client::{kind_label, ModelClient};
use crate::model_policy::ModelTaskKind;

/// Run a fan-out of `count` independent lanes for one `kind` of model task,
/// chunked by `max_parallel`. Returns each lane's storage-safe artifact in
/// stable order.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_lane_group(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    kind: ModelTaskKind,
    generation: usize,
    count: usize,
    max_parallel: usize,
    base_prompt: &str,
    require_parsed_live_json: bool,
) -> Result<Vec<HeroJudgeLaneArtifact>> {
    let mut artifacts = Vec::new();
    let cap = max_parallel
        .clamp(1, crate::superreasoning::MAX_SUPERREASONING_WORKERS)
        .min(count.max(1));
    for lanes in (1..=count.max(1)).collect::<Vec<_>>().chunks(cap) {
        let futures = lanes.iter().map(|lane| {
            let lane = *lane;
            let prompt = format!(
                "{base_prompt}\nLane: {lane}\nReturn exactly one compact JSON object under 700 tokens with summary, claims, questions, rubric, evidence_refs, and score. No markdown, no commentary, and no raw reasoning."
            );
            async move {
                let completion = HeroJudgeCompletionContext {
                    repo,
                    run_id,
                    db,
                    sink,
                    model_client,
                    require_parsed_live_json,
                };
                let (receipt, value) =
                    complete_hero_json(completion, kind, generation, &prompt).await?;
                let summary = summary_from_value(kind, generation, lane, &value);
                let score = score_from_value(kind, generation, &value);
                let metrics = lane_quality_metrics(kind, &value, &summary, score);
                Ok::<_, anyhow::Error>(HeroJudgeLaneArtifact {
                    id: format!("{}-g{generation:03}-l{lane:02}", kind_label(kind)),
                    generation,
                    kind: kind_label(kind).to_string(),
                    lane,
                    model_receipt_id: receipt.id,
                    content_sha256: sha256_json(&value, "hero_judge_artifact"),
                    summary,
                    score,
                    metrics,
                    status: "complete".to_string(),
                })
            }
        });
        for artifact in join_all(futures).await {
            artifacts.push(artifact?);
        }
    }
    artifacts.sort_by_key(|artifact| artifact.lane);
    Ok(artifacts)
}

/// Append a generation-evolution paragraph to a base prompt so model lanes
/// know what the previous generation produced.
pub(crate) fn with_evolution_context(mut prompt: String, context: &str) -> String {
    prompt.push_str("\nEvolution context: ");
    prompt.push_str(context);
    prompt.push_str(" Improve over the retained frontier without inventing evidence.");
    prompt
}

/// Hash of the runbook source. Falls back to a hash of the parsed contents
/// when the path was not readable (e.g. deterministic in-memory tests).
pub(crate) fn source_runbook_sha256(path: &Path, runbook: &HeroJudgeRunbook) -> Result<String> {
    match fs::read(path) {
        Ok(bytes) => Ok(sha256_hex(&bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok(sha256_json(runbook, "hero_judge_runbook"))
        }
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// Build the evolution-context line for a given generation.
pub(crate) fn evolution_context(
    generation: usize,
    decision: &PromotionDecision,
    previous_metric: Option<&HeroJudgeQualityMetric>,
) -> String {
    let Some(metric) = previous_metric else {
        return "Initial generation; establish baseline theory, question, and rubric quality."
            .to_string();
    };
    format!(
        "Previous generation {} frontier prompt {:?}; prior overall {:.3}, frontier {:.3}, theory {:.3}, questions {:.3}, rubric {:.3}. Target measurable gains in the weakest metric while preserving anti-leak and evidence gates.",
        generation.saturating_sub(1),
        decision.winner_prompt_id,
        metric.overall_quality_index,
        metric.frontier_quality_index,
        metric.theory_quality_index,
        metric.question_quality_index,
        metric.rubric_quality_index,
    )
}

/// Filter the per-lane metrics to a single role group, e.g. `"hero"` or
/// `"judge"`.
pub(crate) fn filter_lane_metrics(
    metrics: &[HeroJudgeLaneMetric],
    role_group: &str,
) -> Vec<HeroJudgeLaneMetric> {
    metrics
        .iter()
        .filter(|metric| metric.role_group == role_group)
        .cloned()
        .collect()
}
