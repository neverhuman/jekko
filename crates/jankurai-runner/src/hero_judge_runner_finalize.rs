//! Run-end artifact writer for the Hero/Judge runner. Builds claim ledger,
//! unsupported-claims ledger, negative memory, the storage-safe
//! superreasoning packet, the replay receipt, the reviewer packet, and the
//! headless state files; then runs `validate_completion_artifacts` before
//! writing `complete.ok`. Extracted from `hero_judge_runner_flow` so each
//! file stays under the audit shape threshold.

use std::fs;

use anyhow::{Context, Result};
use serde_json::json;

use crate::daemon_store;
use crate::events::EventKind;
use crate::hero_judge::{HeroJudgeReviewerPacket, HeroJudgeRunSummary};
use crate::hero_judge_eval::{reviewer_questions, write_json_pretty};
use crate::hero_judge_runner_artifacts::{
    artifact_receipts, build_superreasoning_gate_results, replay_source_artifacts,
    validate_completion_artifacts,
};
use crate::superreasoning::{
    SuperReasoningArtifactPaths, SuperReasoningBudgetContract, SuperReasoningPacket,
};

mod artifacts;
mod inputs;
mod ledgers;
mod paths;
mod state;
mod summary;

pub(crate) use inputs::FinalizeInputs;
use paths::RunArtifactPaths;

/// Write all run-end artifacts, run host gate checks, and return the
/// per-run summary on success. On gate failure, the daemon run is marked
/// blocked and an error is returned.
pub(crate) fn finalize_run(inputs: FinalizeInputs<'_>) -> Result<HeroJudgeRunSummary> {
    let FinalizeInputs {
        repo,
        run_id,
        db,
        sink,
        config,
        source_runbook_sha256,
        objective,
        output_dir,
        generations,
        lane_parallelism,
        model_calls_used,
        last_model_kind,
        last_decision,
        prompt_lineage,
        scoreboard,
        knowledge,
        quality_metrics,
        lane_metrics,
        reviewer_cards,
        search_receipts,
    } = inputs;

    let paths = RunArtifactPaths::new(&output_dir);
    let headless = SuperReasoningArtifactPaths::for_run(repo, run_id);
    artifacts::write_core_artifacts(
        &paths,
        run_id,
        &prompt_lineage,
        &scoreboard,
        &last_decision,
        &knowledge,
        &quality_metrics,
        &lane_metrics,
    )?;
    ledgers::write_ledgers(&paths, &headless, run_id, &knowledge, &scoreboard)?;

    let budget_contract = SuperReasoningBudgetContract {
        effective_generations: generations,
        model_call_budget: config.budgets.model_calls,
        search_query_budget: config.budgets.search_queries,
        search_page_budget: config.budgets.search_pages,
        max_parallel: lane_parallelism,
        max_workers: config.super_reasoning.effective_max_workers(),
    };
    let packet = SuperReasoningPacket::hero_judge(
        run_id,
        &objective,
        &config.super_reasoning,
        &output_dir,
        source_runbook_sha256.clone(),
        budget_contract,
        config.model_policy.clone(),
    );
    packet.validate()?;
    write_json_pretty(&paths.output_superreasoning_packet_json, &packet)?;
    write_json_pretty(&headless.superreasoning_packet_json, &packet)?;
    daemon_store::export_model_receipts_jsonl(db, run_id, &headless.model_receipts_jsonl)?;

    let replay_sources = replay_source_artifacts(
        &paths.prompt_lineage_json,
        &paths.frontier_scoreboard_json,
        &paths.promotion_decision_json,
        &paths.knowledge_compound_jsonl,
        &paths.search_receipts_json,
        &paths.quality_metrics_jsonl,
        &paths.lane_metrics_jsonl,
        &paths.output_claim_ledger_jsonl,
        &paths.output_unsupported_claims_jsonl,
        &paths.output_negative_memory_jsonl,
        &paths.output_superreasoning_packet_json,
        &headless,
    );
    let artifact_hashes = artifact_receipts(&replay_sources)?;
    let gate_results = build_superreasoning_gate_results(
        repo,
        &packet,
        &headless,
        &replay_sources,
        model_calls_used,
        config.budgets.model_calls,
    );
    let replay_receipt = packet.replay_receipt(artifact_hashes, gate_results);
    write_json_pretty(&paths.output_replay_receipt_json, &replay_receipt)?;
    write_json_pretty(&headless.replay_receipt_json, &replay_receipt)?;

    let reviewer_packet = HeroJudgeReviewerPacket {
        run_id: run_id.to_string(),
        objective: objective.clone(),
        reviewer_questions: reviewer_questions(),
        quality_metrics: quality_metrics.clone(),
        promotion_decision: last_decision.clone(),
        cards: reviewer_cards,
        superreasoning_packet_path: Some(headless.superreasoning_packet_json.clone()),
        superreasoning_packet_hash: Some(packet.stable_hash.clone()),
        superreasoning_packet: Some(packet.clone()),
        replay_receipt_path: Some(headless.replay_receipt_json.clone()),
        proof_gate_passed: replay_receipt.proof_gate_passed,
        replay_gate_passed: replay_receipt.replay_gate_passed,
        parity_gate_passed: replay_receipt.parity_gate_passed,
        leak_gate_passed: replay_receipt.leak_gate_passed,
        jankurai_gate_passed: replay_receipt.jankurai_gate_passed,
        unsupported_claims_jsonl: Some(headless.unsupported_claims_jsonl.clone()),
        negative_memory_jsonl: Some(headless.negative_memory_jsonl.clone()),
    };
    write_json_pretty(&paths.reviewer_packet_json, &reviewer_packet)?;
    write_json_pretty(&headless.reviewer_packet_json, &reviewer_packet)?;

    let run_state = if replay_receipt.allows_completion() {
        "complete"
    } else {
        "blocked"
    };
    state::write_headless_state(
        &headless,
        run_id,
        run_state,
        &packet,
        &replay_receipt,
        &source_runbook_sha256,
    )?;
    if !replay_receipt.allows_completion() {
        let error = state::gate_error(&replay_receipt);
        daemon_store::mark_daemon_run(db, run_id, "blocked", "superreasoning_gate", Some(&error))?;
        anyhow::bail!("superreasoning gate failed: {error}");
    }
    validate_completion_artifacts(&headless, &replay_receipt, &packet)?;
    fs::write(&paths.complete_ok, b"ok\n")
        .with_context(|| format!("write {}", paths.complete_ok.display()))?;

    let summary = summary::build_summary(summary::SummaryInputs {
        run_id,
        output_dir,
        generations,
        config,
        knowledge_entry_count: knowledge.len(),
        search_receipt_count: search_receipts.len(),
        last_decision,
        model_calls_used,
        last_model_kind,
        paths: &paths,
        headless: &headless,
        packet_hash: packet.stable_hash,
    });
    daemon_store::record_daemon_exit_result(db, run_id, serde_json::to_value(&summary)?)?;
    daemon_store::mark_daemon_run(db, run_id, "complete", "complete", None)?;
    sink.emit(
        EventKind::RunFinished,
        json!({"workflow": "zyal_hero_judge", "status": "complete"}),
    )?;
    Ok(summary)
}
