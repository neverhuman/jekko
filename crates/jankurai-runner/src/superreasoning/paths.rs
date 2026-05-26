use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Paths for headless superreasoning artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningArtifactPaths {
    pub run_dir: PathBuf,
    pub superreasoning_packet_json: PathBuf,
    pub reviewer_packet_json: PathBuf,
    pub replay_receipt_json: PathBuf,
    pub model_receipts_jsonl: PathBuf,
    pub claim_ledger_jsonl: PathBuf,
    pub unsupported_claims_jsonl: PathBuf,
    pub negative_memory_jsonl: PathBuf,
    pub state_json: PathBuf,
    pub state_md: PathBuf,
}

impl SuperReasoningArtifactPaths {
    /// Resolve artifact paths under `target/zyal/runs/<run_id>`.
    pub fn for_run(repo: &Path, run_id: &str) -> Self {
        let run_dir = repo.join("target/zyal/runs").join(run_id);
        Self {
            superreasoning_packet_json: run_dir.join("superreasoning_packet.json"),
            reviewer_packet_json: run_dir.join("reviewer_packet.json"),
            replay_receipt_json: run_dir.join("replay_receipt.json"),
            model_receipts_jsonl: run_dir.join("model_receipts.jsonl"),
            claim_ledger_jsonl: run_dir.join("claim_ledger.jsonl"),
            unsupported_claims_jsonl: run_dir.join("unsupported_claims.jsonl"),
            negative_memory_jsonl: run_dir.join("negative_memory.jsonl"),
            state_json: run_dir.join("STATE.json"),
            state_md: run_dir.join("STATE.md"),
            run_dir,
        }
    }
}
