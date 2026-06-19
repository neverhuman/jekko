use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::classifier;
use crate::hashing::sha256_hex;
use crate::superreasoning::{
    ReplayReceipt, SuperReasoningArtifactPaths, SuperReasoningArtifactReceipt,
    SuperReasoningGateReceipt, SuperReasoningGateResults, SuperReasoningPacket,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn replay_source_artifacts(
    prompt_lineage_json: &Path,
    frontier_scoreboard_json: &Path,
    promotion_decision_json: &Path,
    knowledge_compound_jsonl: &Path,
    search_receipts_json: &Path,
    quality_metrics_jsonl: &Path,
    lane_metrics_jsonl: &Path,
    claim_ledger_jsonl: &Path,
    unsupported_claims_jsonl: &Path,
    negative_memory_jsonl: &Path,
    output_superreasoning_packet_json: &Path,
    headless: &SuperReasoningArtifactPaths,
) -> Vec<PathBuf> {
    vec![
        prompt_lineage_json.to_path_buf(),
        frontier_scoreboard_json.to_path_buf(),
        promotion_decision_json.to_path_buf(),
        knowledge_compound_jsonl.to_path_buf(),
        search_receipts_json.to_path_buf(),
        quality_metrics_jsonl.to_path_buf(),
        lane_metrics_jsonl.to_path_buf(),
        claim_ledger_jsonl.to_path_buf(),
        unsupported_claims_jsonl.to_path_buf(),
        negative_memory_jsonl.to_path_buf(),
        output_superreasoning_packet_json.to_path_buf(),
        headless.superreasoning_packet_json.clone(),
        headless.claim_ledger_jsonl.clone(),
        headless.unsupported_claims_jsonl.clone(),
        headless.negative_memory_jsonl.clone(),
        headless.model_receipts_jsonl.clone(),
    ]
}

pub(crate) fn artifact_receipts(paths: &[PathBuf]) -> Result<Vec<SuperReasoningArtifactReceipt>> {
    paths
        .iter()
        .map(|path| {
            let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
            Ok(SuperReasoningArtifactReceipt {
                path: path.display().to_string(),
                sha256: sha256_hex(&bytes),
            })
        })
        .collect()
}

pub(crate) fn build_superreasoning_gate_results(
    repo: &Path,
    packet: &SuperReasoningPacket,
    headless: &SuperReasoningArtifactPaths,
    replay_sources: &[PathBuf],
    model_calls_used: usize,
    model_call_budget: usize,
) -> SuperReasoningGateResults {
    SuperReasoningGateResults {
        proof_gate: proof_gate(replay_sources, model_calls_used, model_call_budget),
        replay_gate: replay_reconstruction_gate(packet, replay_sources),
        parity_gate: parity_gate(headless),
        leak_gate: leak_gate(packet, replay_sources),
        jankurai_gate: jankurai_gate(repo),
    }
}

fn proof_gate(
    replay_sources: &[PathBuf],
    model_calls_used: usize,
    model_call_budget: usize,
) -> SuperReasoningGateReceipt {
    let mut evidence = replay_sources
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    evidence.push(format!(
        "model_calls_used={model_calls_used};model_call_budget={model_call_budget}"
    ));
    if model_calls_used > model_call_budget {
        return SuperReasoningGateReceipt::failed(
            format!("model calls used {model_calls_used} exceed budget {model_call_budget}"),
            evidence,
        );
    }
    let missing = replay_sources
        .iter()
        .filter(|path| !path.exists())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return SuperReasoningGateReceipt::failed(
            format!("missing proof artifacts: {}", missing.join(", ")),
            evidence,
        );
    }
    SuperReasoningGateReceipt::passed(evidence)
}

fn replay_reconstruction_gate(
    packet: &SuperReasoningPacket,
    replay_sources: &[PathBuf],
) -> SuperReasoningGateReceipt {
    let evidence = replay_sources
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    for path in replay_sources {
        if let Err(err) = validate_replay_source(path, packet) {
            return SuperReasoningGateReceipt::failed(err.to_string(), evidence);
        }
    }
    SuperReasoningGateReceipt::passed(evidence)
}

fn validate_replay_source(path: &Path, expected_packet: &SuperReasoningPacket) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => {
            if file_name == "superreasoning_packet.json" {
                let packet: SuperReasoningPacket =
                    serde_json::from_slice(&bytes).context("parse superreasoning packet")?;
                packet.validate()?;
                if packet.stable_hash != expected_packet.stable_hash {
                    anyhow::bail!("superreasoning packet hash changed during replay");
                }
            } else {
                let _: serde_json::Value = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parse replay JSON artifact {}", path.display()))?;
            }
        }
        Some("jsonl") => {
            let text = std::str::from_utf8(&bytes)
                .with_context(|| format!("utf8 replay JSONL artifact {}", path.display()))?;
            let allow_empty = file_name == "unsupported_claims.jsonl";
            let mut line_count = 0_usize;
            for (idx, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let _: serde_json::Value = serde_json::from_str(line).with_context(|| {
                    format!(
                        "parse replay JSONL artifact {} line {}",
                        path.display(),
                        idx + 1
                    )
                })?;
                line_count += 1;
            }
            if !allow_empty && line_count == 0 {
                anyhow::bail!("required replay JSONL artifact {} is empty", path.display());
            }
        }
        _ => {}
    }
    Ok(())
}

fn parity_gate(headless: &SuperReasoningArtifactPaths) -> SuperReasoningGateReceipt {
    let summary_path = headless.run_dir.join("parity/summary.json");
    if !summary_path.exists() {
        return SuperReasoningGateReceipt::not_applicable(
            "hero/judge run has no parity target summary artifact",
            vec![summary_path.display().to_string()],
        );
    }
    let evidence = vec![summary_path.display().to_string()];
    let parsed = fs::read_to_string(&summary_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let Some(value) = parsed else {
        return SuperReasoningGateReceipt::failed("parity summary is not valid JSON", evidence);
    };
    let status_passed = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(|status| status == "passed")
        .unwrap_or(false);
    let gaps_empty = value
        .get("gaps")
        .and_then(serde_json::Value::as_array)
        .map(|gaps| gaps.is_empty())
        .unwrap_or(true);
    if status_passed && gaps_empty {
        SuperReasoningGateReceipt::passed(evidence)
    } else {
        SuperReasoningGateReceipt::failed("parity summary did not pass", evidence)
    }
}

fn leak_gate(
    packet: &SuperReasoningPacket,
    replay_sources: &[PathBuf],
) -> SuperReasoningGateReceipt {
    let evidence = replay_sources
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let forbidden = packet
        .artifact_contract
        .forbidden_content
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for path in replay_sources {
        if path.file_name().and_then(|name| name.to_str()) == Some("superreasoning_packet.json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            return SuperReasoningGateReceipt::failed(
                format!("could not scan {} for forbidden content", path.display()),
                evidence,
            );
        };
        let lower = text.to_ascii_lowercase();
        if let Some(term) = forbidden.iter().find(|term| lower.contains(term.as_str())) {
            return SuperReasoningGateReceipt::failed(
                format!(
                    "forbidden content marker {term:?} found in {}",
                    path.display()
                ),
                evidence,
            );
        }
    }
    SuperReasoningGateReceipt::passed(evidence)
}

fn jankurai_gate(repo: &Path) -> SuperReasoningGateReceipt {
    let score_path = repo.join(".jankurai/repo-score.json");
    if !score_path.exists() {
        return SuperReasoningGateReceipt::not_applicable(
            "no local Jankurai score artifact was present for this deterministic run",
            vec![score_path.display().to_string()],
        );
    }
    let evidence = vec![score_path.display().to_string()];
    match classifier::classify(repo) {
        Ok(result) if result.decision_passed == Some(true) => {
            let mut evidence = evidence;
            evidence.push(format!("score={:.0}", result.score));
            if let Some(status) = result.decision_status {
                evidence.push(format!("decision_status={status}"));
            }
            evidence.push(format!("hard_findings={}", result.hard_total));
            evidence.push(format!("caps={}", result.caps_total));
            SuperReasoningGateReceipt::passed(evidence)
        }
        Ok(result)
            if result.decision_passed.is_none()
                && result.hard_total == 0
                && result.caps_total == 0 =>
        {
            let mut evidence = evidence;
            evidence.push(format!("score={:.0}", result.score));
            SuperReasoningGateReceipt::passed(evidence)
        }
        Ok(result) => SuperReasoningGateReceipt::failed(
            format!(
                "Jankurai gate failed with hard_findings={} caps={}",
                result.hard_total, result.caps_total
            ),
            evidence,
        ),
        Err(err) => SuperReasoningGateReceipt::failed(err.to_string(), evidence),
    }
}

pub(crate) fn validate_completion_artifacts(
    headless: &SuperReasoningArtifactPaths,
    replay_receipt: &ReplayReceipt,
    expected_packet: &SuperReasoningPacket,
) -> Result<()> {
    for path in [
        &headless.superreasoning_packet_json,
        &headless.reviewer_packet_json,
        &headless.replay_receipt_json,
        &headless.model_receipts_jsonl,
        &headless.claim_ledger_jsonl,
        &headless.unsupported_claims_jsonl,
        &headless.negative_memory_jsonl,
        &headless.state_json,
        &headless.state_md,
    ] {
        if !path.exists() {
            anyhow::bail!("completion artifact missing: {}", path.display());
        }
    }
    let persisted: ReplayReceipt =
        serde_json::from_slice(&fs::read(&headless.replay_receipt_json)?)
            .with_context(|| format!("parse {}", headless.replay_receipt_json.display()))?;
    if persisted != *replay_receipt {
        anyhow::bail!("persisted replay receipt does not match host receipt");
    }
    if !persisted.allows_completion() {
        anyhow::bail!("replay receipt does not allow completion");
    }
    let reconstructed =
        SuperReasoningPacket::reconstruct_from_artifact(&headless.superreasoning_packet_json)?;
    if reconstructed.stable_hash != expected_packet.stable_hash {
        anyhow::bail!(
            "reconstructed superreasoning packet hash {} does not match host hash {}",
            reconstructed.stable_hash,
            expected_packet.stable_hash
        );
    }
    persisted.verify_artifact_integrity()?;
    Ok(())
}
