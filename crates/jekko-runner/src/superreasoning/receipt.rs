use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::hashing::sha256_hex;

/// One artifact hash captured by the replay receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningArtifactReceipt {
    pub path: String,
    pub sha256: String,
}

/// Host-derived gate receipt. `not_applicable` is an explicit host policy,
/// not a silent pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningGateReceipt {
    pub status: String,
    pub required: bool,
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuperReasoningGateReceipt {
    pub fn passed(evidence: Vec<String>) -> Self {
        Self {
            status: "passed".to_string(),
            required: true,
            evidence,
            message: None,
        }
    }

    pub fn failed(message: impl Into<String>, evidence: Vec<String>) -> Self {
        Self {
            status: "failed".to_string(),
            required: true,
            evidence,
            message: Some(message.into()),
        }
    }

    pub fn not_applicable(message: impl Into<String>, evidence: Vec<String>) -> Self {
        Self {
            status: "not_applicable".to_string(),
            required: false,
            evidence,
            message: Some(message.into()),
        }
    }

    pub fn allows_completion(&self) -> bool {
        matches!(self.status.as_str(), "passed" | "not_applicable")
    }
}

/// All promotion gates captured in the final replay receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningGateResults {
    pub proof_gate: SuperReasoningGateReceipt,
    pub replay_gate: SuperReasoningGateReceipt,
    pub parity_gate: SuperReasoningGateReceipt,
    pub leak_gate: SuperReasoningGateReceipt,
    pub jankurai_gate: SuperReasoningGateReceipt,
}

impl SuperReasoningGateResults {
    pub fn allows_completion(&self) -> bool {
        self.proof_gate.allows_completion()
            && self.replay_gate.allows_completion()
            && self.parity_gate.allows_completion()
            && self.leak_gate.allows_completion()
            && self.jankurai_gate.allows_completion()
    }
}

/// Replay receipt schema shared by packet and headless artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayReceipt {
    pub schema_version: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_runbook_sha256: Option<String>,
    #[serde(default)]
    pub artifact_hashes: Vec<SuperReasoningArtifactReceipt>,
    pub gate_results: SuperReasoningGateResults,
    pub replay_gate_passed: bool,
    pub parity_gate_passed: bool,
    pub leak_gate_passed: bool,
    pub jankurai_gate_passed: bool,
    pub proof_gate_passed: bool,
    pub status: String,
}

impl ReplayReceipt {
    /// Build a pending receipt embedded in the immutable packet.
    pub fn pending(
        run_id: &str,
        packet_hash: Option<String>,
        policy_hash: Option<String>,
        source_runbook_sha256: Option<String>,
    ) -> Self {
        let pending = SuperReasoningGateReceipt {
            status: "pending".to_string(),
            required: true,
            evidence: Vec::new(),
            message: Some("final receipt is derived after host replay".to_string()),
        };
        Self {
            schema_version: "zyal.superreasoning.replay_receipt.v1".to_string(),
            run_id: run_id.to_string(),
            packet_hash,
            policy_hash,
            source_runbook_sha256,
            artifact_hashes: Vec::new(),
            gate_results: SuperReasoningGateResults {
                proof_gate: pending.clone(),
                replay_gate: pending.clone(),
                parity_gate: pending.clone(),
                leak_gate: pending.clone(),
                jankurai_gate: pending,
            },
            replay_gate_passed: false,
            parity_gate_passed: false,
            leak_gate_passed: false,
            jankurai_gate_passed: false,
            proof_gate_passed: false,
            status: "pending".to_string(),
        }
    }

    /// Build the final receipt from host-derived gates and artifact hashes.
    pub fn from_gate_results(
        run_id: &str,
        packet_hash: String,
        policy_hash: String,
        source_runbook_sha256: String,
        artifact_hashes: Vec<SuperReasoningArtifactReceipt>,
        gate_results: SuperReasoningGateResults,
    ) -> Self {
        let status = if gate_results.allows_completion() {
            "passed"
        } else {
            "failed"
        };
        Self {
            schema_version: "zyal.superreasoning.replay_receipt.v1".to_string(),
            run_id: run_id.to_string(),
            packet_hash: Some(packet_hash),
            policy_hash: Some(policy_hash),
            source_runbook_sha256: Some(source_runbook_sha256),
            artifact_hashes,
            replay_gate_passed: gate_results.replay_gate.allows_completion(),
            parity_gate_passed: gate_results.parity_gate.allows_completion(),
            leak_gate_passed: gate_results.leak_gate.allows_completion(),
            jankurai_gate_passed: gate_results.jankurai_gate.allows_completion(),
            proof_gate_passed: gate_results.proof_gate.allows_completion(),
            gate_results,
            status: status.to_string(),
        }
    }

    pub fn allows_completion(&self) -> bool {
        self.status == "passed" && self.gate_results.allows_completion()
    }

    /// Re-hash every artifact path recorded in this receipt and confirm the
    /// stored sha256 still matches the file on disk. Any tampering or missing
    /// artifact surfaces as an explicit error here so that
    /// `validate_completion_artifacts` can refuse to write `complete.ok`.
    pub fn verify_artifact_integrity(&self) -> Result<()> {
        for artifact in &self.artifact_hashes {
            let path = Path::new(&artifact.path);
            let bytes = fs::read(path)
                .with_context(|| format!("read receipted artifact {}", artifact.path))?;
            let actual = sha256_hex(&bytes);
            if actual != artifact.sha256 {
                return Err(anyhow!(
                    "replay receipt artifact hash mismatch for {}: recorded {}, observed {}",
                    artifact.path,
                    artifact.sha256,
                    actual
                ));
            }
        }
        Ok(())
    }
}
