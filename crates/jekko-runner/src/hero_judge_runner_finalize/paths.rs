use std::path::{Path, PathBuf};

pub(super) struct RunArtifactPaths {
    pub prompt_lineage_json: PathBuf,
    pub frontier_scoreboard_json: PathBuf,
    pub promotion_decision_json: PathBuf,
    pub knowledge_compound_jsonl: PathBuf,
    pub search_receipts_json: PathBuf,
    pub quality_metrics_jsonl: PathBuf,
    pub quality_metrics_csv: PathBuf,
    pub quality_trend_json: PathBuf,
    pub lane_metrics_jsonl: PathBuf,
    pub lane_metrics_csv: PathBuf,
    pub hero_metrics_csv: PathBuf,
    pub judge_metrics_csv: PathBuf,
    pub reviewer_packet_json: PathBuf,
    pub output_superreasoning_packet_json: PathBuf,
    pub output_replay_receipt_json: PathBuf,
    pub output_claim_ledger_jsonl: PathBuf,
    pub output_unsupported_claims_jsonl: PathBuf,
    pub output_negative_memory_jsonl: PathBuf,
    pub complete_ok: PathBuf,
}

impl RunArtifactPaths {
    pub fn new(output_dir: &Path) -> Self {
        Self {
            prompt_lineage_json: output_dir.join("prompt_lineage.json"),
            frontier_scoreboard_json: output_dir.join("frontier_scoreboard.json"),
            promotion_decision_json: output_dir.join("promotion-decision.json"),
            knowledge_compound_jsonl: output_dir.join("knowledge_compound.jsonl"),
            search_receipts_json: output_dir.join("search").join("receipts.json"),
            quality_metrics_jsonl: output_dir.join("quality_metrics.jsonl"),
            quality_metrics_csv: output_dir.join("quality_metrics.csv"),
            quality_trend_json: output_dir.join("quality_trend.json"),
            lane_metrics_jsonl: output_dir.join("lane_metrics.jsonl"),
            lane_metrics_csv: output_dir.join("lane_metrics.csv"),
            hero_metrics_csv: output_dir.join("hero_metrics.csv"),
            judge_metrics_csv: output_dir.join("judge_metrics.csv"),
            reviewer_packet_json: output_dir.join("reviewer_packet.json"),
            output_superreasoning_packet_json: output_dir.join("superreasoning_packet.json"),
            output_replay_receipt_json: output_dir.join("replay_receipt.json"),
            output_claim_ledger_jsonl: output_dir.join("claim_ledger.jsonl"),
            output_unsupported_claims_jsonl: output_dir.join("unsupported_claims.jsonl"),
            output_negative_memory_jsonl: output_dir.join("negative_memory.jsonl"),
            complete_ok: output_dir.join("complete.ok"),
        }
    }
}
