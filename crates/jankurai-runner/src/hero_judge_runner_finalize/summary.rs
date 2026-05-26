use std::path::PathBuf;

use crate::hero_judge::{HeroJudgeConfig, HeroJudgeRunSummary, PromotionDecision};
use crate::superreasoning::SuperReasoningArtifactPaths;

use super::paths::RunArtifactPaths;

pub(super) struct SummaryInputs<'a> {
    pub run_id: &'a str,
    pub output_dir: PathBuf,
    pub generations: usize,
    pub config: &'a HeroJudgeConfig,
    pub knowledge_entry_count: usize,
    pub search_receipt_count: usize,
    pub last_decision: PromotionDecision,
    pub model_calls_used: usize,
    pub last_model_kind: Option<String>,
    pub paths: &'a RunArtifactPaths,
    pub headless: &'a SuperReasoningArtifactPaths,
    pub packet_hash: String,
}

pub(super) fn build_summary(inputs: SummaryInputs<'_>) -> HeroJudgeRunSummary {
    let SummaryInputs {
        run_id,
        output_dir,
        generations,
        config,
        knowledge_entry_count,
        search_receipt_count,
        last_decision,
        model_calls_used,
        last_model_kind,
        paths,
        headless,
        packet_hash,
    } = inputs;
    HeroJudgeRunSummary {
        run_id: run_id.to_string(),
        output_dir,
        generation: generations,
        hero_lane_count: config.population.hero_lanes,
        judge_lane_count: config.population.judge_lanes,
        frontier_winner: last_decision.winner_candidate_id.clone(),
        knowledge_entry_count,
        search_receipt_count,
        last_promotion_decision: last_decision,
        model_calls_used,
        model_call_budget: config.budgets.model_calls,
        last_model_kind,
        prompt_lineage_json: paths.prompt_lineage_json.clone(),
        frontier_scoreboard_json: paths.frontier_scoreboard_json.clone(),
        promotion_decision_json: paths.promotion_decision_json.clone(),
        knowledge_compound_jsonl: paths.knowledge_compound_jsonl.clone(),
        search_receipts_json: paths.search_receipts_json.clone(),
        quality_metrics_jsonl: paths.quality_metrics_jsonl.clone(),
        quality_metrics_csv: paths.quality_metrics_csv.clone(),
        quality_trend_json: paths.quality_trend_json.clone(),
        lane_metrics_jsonl: paths.lane_metrics_jsonl.clone(),
        lane_metrics_csv: paths.lane_metrics_csv.clone(),
        hero_metrics_csv: paths.hero_metrics_csv.clone(),
        judge_metrics_csv: paths.judge_metrics_csv.clone(),
        reviewer_packet_json: paths.reviewer_packet_json.clone(),
        superreasoning_packet_json: headless.superreasoning_packet_json.clone(),
        superreasoning_packet_sha256: packet_hash,
        replay_receipt_json: headless.replay_receipt_json.clone(),
        model_receipts_jsonl: headless.model_receipts_jsonl.clone(),
        claim_ledger_jsonl: headless.claim_ledger_jsonl.clone(),
        unsupported_claims_jsonl: headless.unsupported_claims_jsonl.clone(),
        negative_memory_jsonl: headless.negative_memory_jsonl.clone(),
        headless_state_json: headless.state_json.clone(),
        headless_state_md: headless.state_md.clone(),
        complete_ok: paths.complete_ok.clone(),
    }
}
