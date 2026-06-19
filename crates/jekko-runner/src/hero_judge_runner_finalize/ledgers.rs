use anyhow::Result;
use serde_json::json;

use crate::hero_judge::{FrontierScore, KnowledgeEntry};
use crate::hero_judge_eval::write_jsonl;
use crate::superreasoning::SuperReasoningArtifactPaths;

use super::paths::RunArtifactPaths;

pub(super) fn write_ledgers(
    paths: &RunArtifactPaths,
    headless: &SuperReasoningArtifactPaths,
    run_id: &str,
    knowledge: &[KnowledgeEntry],
    scoreboard: &[FrontierScore],
) -> Result<()> {
    let claim_ledger = knowledge
        .iter()
        .map(|entry| {
            json!({
                "schema_version": "zyal.superreasoning.claim.v1",
                "id": entry.id,
                "status": entry.status,
                "claim": entry.claim,
                "evidence_refs": entry.evidence_refs,
                "content_sha256": entry.content_sha256,
            })
        })
        .collect::<Vec<_>>();
    let unsupported_claims = scoreboard
        .iter()
        .filter(|score| score.status != "promoted" || score.leak_status != "clean")
        .map(|score| {
            json!({
                "schema_version": "zyal.superreasoning.unsupported_claim.v1",
                "candidate_id": score.candidate_id,
                "generation": score.generation,
                "score": score.score,
                "reason": score.status,
                "leak_status": score.leak_status,
            })
        })
        .collect::<Vec<_>>();
    let negative_memory = negative_memory_entries(
        run_id,
        scoreboard,
        &paths.prompt_lineage_json.display().to_string(),
        &paths.frontier_scoreboard_json.display().to_string(),
        &paths.quality_metrics_jsonl.display().to_string(),
    );

    write_jsonl(&paths.output_claim_ledger_jsonl, &claim_ledger)?;
    write_jsonl(&paths.output_unsupported_claims_jsonl, &unsupported_claims)?;
    write_jsonl(&paths.output_negative_memory_jsonl, &negative_memory)?;
    write_jsonl(&headless.claim_ledger_jsonl, &claim_ledger)?;
    write_jsonl(&headless.unsupported_claims_jsonl, &unsupported_claims)?;
    write_jsonl(&headless.negative_memory_jsonl, &negative_memory)?;
    Ok(())
}

fn negative_memory_entries(
    run_id: &str,
    scoreboard: &[FrontierScore],
    prompt_lineage_path: &str,
    frontier_scoreboard_path: &str,
    quality_metrics_path: &str,
) -> Vec<serde_json::Value> {
    let mut negative_memory: Vec<serde_json::Value> = scoreboard
        .iter()
        .filter(|score| score.status != "promoted" || score.leak_status != "clean")
        .map(|score| {
            let cause = if score.leak_status != "clean" {
                format!("leak_status={}", score.leak_status)
            } else {
                format!("rejected_status={}", score.status)
            };
            json!({
                "schema_version": "zyal.superreasoning.negative_memory.v1",
                "run_id": run_id,
                "id": format!("neg-g{:03}-{}", score.generation, score.candidate_id),
                "kind": "candidate_rejection",
                "candidate_id": score.candidate_id,
                "prompt_id": score.prompt_id,
                "generation": score.generation,
                "score": score.score,
                "verifier_score": score.verifier_score,
                "red_team_penalty": score.red_team_penalty,
                "leak_status": score.leak_status,
                "summary": format!("Do not re-promote {}; {}.", score.candidate_id, cause),
                "status": "verified",
                "evidence_refs": [
                    prompt_lineage_path,
                    frontier_scoreboard_path,
                    quality_metrics_path,
                ],
            })
        })
        .collect();
    negative_memory.push(json!({
        "schema_version": "zyal.superreasoning.negative_memory.v1",
        "run_id": run_id,
        "id": format!("neg-policy-{run_id}"),
        "kind": "policy_invariant",
        "summary": "Do not promote claims without evidence, replay, parity, leak, and Jankurai gates.",
        "status": "verified",
        "evidence_refs": [
            prompt_lineage_path,
            frontier_scoreboard_path,
            quality_metrics_path,
        ],
    }));
    negative_memory
}
