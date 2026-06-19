//! Independent offline verifier for completed ZYAL superreasoning runs.
//!
//! Given a run directory containing `superreasoning_packet.json` and
//! `replay_receipt.json`, this module re-hashes every artifact named in the
//! receipt, confirms the packet's recorded hashes match the receipt's
//! cross-references, and reports per-gate status. It deliberately does not
//! depend on `jekko-runner` — running the producer and verifier from the
//! same crate undermines the third-party-reproducibility guarantee the
//! superreasoning packet is supposed to provide.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const PACKET_SCHEMA: &str = "zyal.superreasoning.packet.v1";
const RECEIPT_SCHEMA: &str = "zyal.superreasoning.replay_receipt.v1";
const PACKET_FILE: &str = "superreasoning_packet.json";
const RECEIPT_FILE: &str = "replay_receipt.json";

/// One line of receipt evidence summarized for the report.
#[derive(Debug, Clone, Serialize)]
pub struct GateSummary {
    pub name: String,
    pub status: String,
    pub required: bool,
    pub evidence_count: usize,
    pub message: Option<String>,
}

/// Verification report.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub schema_version: String,
    pub run_dir: PathBuf,
    pub packet_path: PathBuf,
    pub receipt_path: PathBuf,
    pub packet_hash: Option<String>,
    pub receipt_packet_hash: Option<String>,
    pub policy_hash: Option<String>,
    pub source_runbook_sha256: Option<String>,
    pub artifact_count: usize,
    pub artifact_hash_mismatches: Vec<String>,
    pub missing_artifacts: Vec<String>,
    pub gates: Vec<GateSummary>,
    pub status: String,
    pub failures: Vec<String>,
}

impl VerifyReport {
    /// Exit 0 when verification passes, 1 otherwise.
    pub fn exit_code(&self) -> i32 {
        if self.status == "passed" {
            0
        } else {
            1
        }
    }
}

/// Verify a completed superreasoning run directory.
pub fn verify(run_dir: &Path) -> Result<VerifyReport> {
    let packet_path = run_dir.join(PACKET_FILE);
    let receipt_path = run_dir.join(RECEIPT_FILE);
    let mut report = VerifyReport {
        schema_version: "zyal.superreasoning.verify.v1".to_string(),
        run_dir: run_dir.to_path_buf(),
        packet_path: packet_path.clone(),
        receipt_path: receipt_path.clone(),
        packet_hash: None,
        receipt_packet_hash: None,
        policy_hash: None,
        source_runbook_sha256: None,
        artifact_count: 0,
        artifact_hash_mismatches: Vec::new(),
        missing_artifacts: Vec::new(),
        gates: Vec::new(),
        status: "failed".to_string(),
        failures: Vec::new(),
    };

    let packet = load_json(&packet_path)?;
    check_string_field(
        &packet,
        "schema_version",
        PACKET_SCHEMA,
        &mut report.failures,
    );
    if let Some(privacy) = packet.get("privacy_contract") {
        let store_raw = privacy
            .get("store_raw_reasoning")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if store_raw {
            report.failures.push(
                "privacy_contract.store_raw_reasoning is true; packet must forbid raw reasoning"
                    .to_string(),
            );
        }
        let users_only = privacy
            .get("users_only_credentials")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !users_only {
            report
                .failures
                .push("privacy_contract.users_only_credentials must be true".to_string());
        }
    } else {
        report
            .failures
            .push("packet is missing privacy_contract".to_string());
    }
    if let Some(lanes) = packet.get("lane_plan").and_then(|v| v.as_array()) {
        for lane in lanes {
            if let Some(workers) = lane.get("max_workers").and_then(|v| v.as_u64()) {
                if workers > 10 {
                    let id = lane
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>");
                    report
                        .failures
                        .push(format!("lane {id} max_workers={workers} exceeds 10"));
                }
            }
        }
    }
    report.packet_hash = packet
        .get("stable_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    report.policy_hash = packet
        .get("policy_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    report.source_runbook_sha256 = packet
        .get("source_runbook_sha256")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let receipt = load_json(&receipt_path)?;
    check_string_field(
        &receipt,
        "schema_version",
        RECEIPT_SCHEMA,
        &mut report.failures,
    );
    report.receipt_packet_hash = receipt
        .get("packet_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let receipt_policy = receipt
        .get("policy_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let receipt_source = receipt
        .get("source_runbook_sha256")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let receipt_status = receipt.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if receipt_status != "passed" {
        report.failures.push(format!(
            "replay receipt status is {receipt_status}, expected passed"
        ));
    }
    if report.packet_hash.is_some() && report.packet_hash != report.receipt_packet_hash {
        report.failures.push(format!(
            "packet stable_hash {:?} does not match receipt.packet_hash {:?}",
            report.packet_hash, report.receipt_packet_hash
        ));
    }
    if report.policy_hash.is_some() && report.policy_hash != receipt_policy {
        report.failures.push(format!(
            "packet.policy_hash {:?} does not match receipt.policy_hash {:?}",
            report.policy_hash, receipt_policy
        ));
    }
    if report.source_runbook_sha256.is_some() && report.source_runbook_sha256 != receipt_source {
        report.failures.push(format!(
            "packet.source_runbook_sha256 {:?} does not match receipt.source_runbook_sha256 {:?}",
            report.source_runbook_sha256, receipt_source
        ));
    }

    if let Some(gates) = receipt.get("gate_results") {
        for name in [
            "proof_gate",
            "replay_gate",
            "parity_gate",
            "leak_gate",
            "jankurai_gate",
        ] {
            let Some(gate) = gates.get(name) else {
                report.failures.push(format!("missing gate {name}"));
                continue;
            };
            let status = gate
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("missing")
                .to_string();
            let required = gate
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let evidence_count = gate
                .get("evidence")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            let message = gate
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if !matches!(status.as_str(), "passed" | "not_applicable") {
                report
                    .failures
                    .push(format!("gate {name} status is {status}, not allowed"));
            }
            report.gates.push(GateSummary {
                name: name.to_string(),
                status,
                required,
                evidence_count,
                message,
            });
        }
    } else {
        report.failures.push("receipt missing gate_results".into());
    }

    if let Some(artifacts) = receipt.get("artifact_hashes").and_then(|v| v.as_array()) {
        report.artifact_count = artifacts.len();
        for entry in artifacts {
            let Some(path_str) = entry.get("path").and_then(|v| v.as_str()) else {
                report
                    .failures
                    .push("artifact entry missing path".to_string());
                continue;
            };
            let Some(expected) = entry.get("sha256").and_then(|v| v.as_str()) else {
                report
                    .failures
                    .push(format!("artifact {path_str} missing sha256"));
                continue;
            };
            match fs::read(path_str) {
                Ok(bytes) => {
                    let actual = sha256_hex(&bytes);
                    if actual != expected {
                        report.artifact_hash_mismatches.push(path_str.to_string());
                        report.failures.push(format!(
                            "artifact {path_str} hash mismatch: recorded {expected}, observed {actual}"
                        ));
                    }
                }
                Err(_) => {
                    report.missing_artifacts.push(path_str.to_string());
                    report
                        .failures
                        .push(format!("artifact {path_str} could not be read"));
                }
            }
        }
    } else {
        report
            .failures
            .push("receipt missing artifact_hashes".to_string());
    }

    if report.failures.is_empty() {
        report.status = "passed".to_string();
    }
    Ok(report)
}

fn load_json(path: &Path) -> Result<serde_json::Value> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn check_string_field(
    value: &serde_json::Value,
    field: &str,
    expected: &str,
    failures: &mut Vec<String>,
) {
    let Some(found) = value.get(field).and_then(|v| v.as_str()) else {
        failures.push(format!("missing {field}"));
        return;
    };
    if found != expected {
        failures.push(format!("{field}={found}, expected {expected}"));
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Bail-out helper for callers that want an error rather than a report.
pub fn verify_strict(run_dir: &Path) -> Result<VerifyReport> {
    let report = verify(run_dir)?;
    if report.status != "passed" {
        return Err(anyhow!(
            "replay verification failed: {}",
            report.failures.join("; ")
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
