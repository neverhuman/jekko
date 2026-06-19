use std::fs;

use anyhow::{Context, Result};
use serde_json::json;

use crate::superreasoning::{ReplayReceipt, SuperReasoningArtifactPaths, SuperReasoningPacket};

pub(super) fn write_headless_state(
    headless: &SuperReasoningArtifactPaths,
    run_id: &str,
    run_state: &str,
    packet: &SuperReasoningPacket,
    replay_receipt: &ReplayReceipt,
    source_runbook_sha256: &str,
) -> Result<()> {
    let headless_state = json!({
        "schema_version": "zyal.superreasoning.state.v1",
        "run_id": run_id,
        "state": run_state,
        "packet_hash": packet.stable_hash,
        "policy_hash": packet.policy_hash,
        "source_runbook_sha256": source_runbook_sha256,
        "gates": replay_receipt.gate_results.clone(),
        "artifacts": {
            "events": headless.run_dir.join("events.jsonl").display().to_string(),
            "superreasoning_packet": headless.superreasoning_packet_json.display().to_string(),
            "reviewer_packet": headless.reviewer_packet_json.display().to_string(),
            "replay_receipt": headless.replay_receipt_json.display().to_string(),
            "claim_ledger": headless.claim_ledger_jsonl.display().to_string(),
            "unsupported_claims": headless.unsupported_claims_jsonl.display().to_string(),
            "negative_memory": headless.negative_memory_jsonl.display().to_string(),
        }
    });
    crate::hero_judge_eval::write_json_pretty(&headless.state_json, &headless_state)?;
    if let Some(parent) = headless.state_md.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(
        &headless.state_md,
        format!(
            "# ZYAL Superreasoning State\n\nrun_id: {run_id}\nstate: {run_state}\npacket_hash: {}\npolicy_hash: {}\n",
            packet.stable_hash, packet.policy_hash
        ),
    )
    .with_context(|| format!("write {}", headless.state_md.display()))?;
    Ok(())
}

pub(super) fn gate_error(replay_receipt: &ReplayReceipt) -> String {
    replay_receipt
        .gate_results
        .proof_gate
        .message
        .as_deref()
        .or(replay_receipt.gate_results.replay_gate.message.as_deref())
        .or(replay_receipt.gate_results.leak_gate.message.as_deref())
        .or(replay_receipt.gate_results.jankurai_gate.message.as_deref())
        .or(replay_receipt.gate_results.parity_gate.message.as_deref())
        .unwrap_or("superreasoning gate failed")
        .to_string()
}
