use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::hashing::sha256_json;
use crate::model_client::CredentialSourcePolicy;
use crate::model_policy::ModelPolicy;

use super::{
    ReplayReceipt, SuperReasoningArtifactReceipt, SuperReasoningConfig, SuperReasoningGateResults,
    MAX_SUPERREASONING_WORKERS,
};

/// One planned reasoning lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningLanePlan {
    pub id: String,
    pub role: String,
    pub route: String,
    pub max_workers: usize,
    pub required_artifacts: Vec<String>,
}

/// Required artifact contract. Canonical type lives in `zyal-core`; aliased
/// here so existing `SuperReasoningArtifactContract` paths keep compiling.
pub use zyal_core::ArtifactContract as SuperReasoningArtifactContract;

/// Privacy contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningPrivacyContract {
    pub store_raw_reasoning: bool,
    pub users_only_credentials: bool,
    pub model_visible_target_values: bool,
    pub storage_safe_summaries_only: bool,
}

/// Promotion gates that must pass before completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningPromotionGates {
    pub proof_gate: bool,
    pub replay_gate: bool,
    pub parity_gate: bool,
    pub leak_gate: bool,
    pub jankurai_gate: bool,
}

/// Immutable budget contract used to route and audit model work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningBudgetContract {
    pub effective_generations: usize,
    pub model_call_budget: usize,
    pub search_query_budget: usize,
    pub search_page_budget: usize,
    pub max_parallel: usize,
    pub max_workers: usize,
}

/// Storage-safe packet handed to independent reviewers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperReasoningPacket {
    pub schema_version: String,
    pub run_id: String,
    pub objective: String,
    pub source_runbook_sha256: String,
    pub effective_generations: usize,
    pub budget_contract: SuperReasoningBudgetContract,
    pub model_route_contract: ModelPolicy,
    pub lane_plan: Vec<SuperReasoningLanePlan>,
    pub artifact_contract: SuperReasoningArtifactContract,
    pub privacy_contract: SuperReasoningPrivacyContract,
    pub promotion_gates: SuperReasoningPromotionGates,
    pub credential_policy: CredentialSourcePolicy,
    pub policy_hash: String,
    pub replay_receipt: ReplayReceipt,
    pub stable_hash: String,
}

impl SuperReasoningPacket {
    /// Build a packet for a Hero/Judge superreasoning run.
    pub fn hero_judge(
        run_id: &str,
        objective: &str,
        config: &SuperReasoningConfig,
        output_dir: &Path,
        source_runbook_sha256: String,
        budget_contract: SuperReasoningBudgetContract,
        model_policy: ModelPolicy,
    ) -> Self {
        let max_workers = config.effective_max_workers();
        let artifact_contract = SuperReasoningArtifactContract {
            required_artifacts: vec![
                "superreasoning_packet.json".to_string(),
                "reviewer_packet.json".to_string(),
                "replay_receipt.json".to_string(),
                "model_receipts.jsonl".to_string(),
                "claim_ledger.jsonl".to_string(),
                "unsupported_claims.jsonl".to_string(),
                "negative_memory.jsonl".to_string(),
            ],
            forbidden_content: zyal_core::FORBIDDEN_ARTIFACT_SHAPE_PATTERNS
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
            claim_ledger: output_dir.join("claim_ledger.jsonl").display().to_string(),
            unsupported_claims_ledger: output_dir
                .join("unsupported_claims.jsonl")
                .display()
                .to_string(),
            negative_memory: output_dir
                .join("negative_memory.jsonl")
                .display()
                .to_string(),
        };
        let lane_plan = [
            ("literature", "research", "routine"),
            ("prior_art", "prior_art_checker", "routine"),
            ("hero", "candidate_generator", "routine"),
            ("judge", "critic", "critic"),
            ("verifier", "verifier", "verifier"),
            ("red_team", "adversary", "critic"),
            ("replay_reproducer", "reproducer", "reproducer"),
            ("parity_mapper", "parity", "reproducer"),
            ("meta_judge", "reducer", "meta_judge"),
            ("memory_curator", "memory", "memory_curator"),
        ]
        .into_iter()
        .map(|(id, role, route)| SuperReasoningLanePlan {
            id: id.to_string(),
            role: role.to_string(),
            route: route.to_string(),
            max_workers,
            required_artifacts: artifact_contract.required_artifacts.clone(),
        })
        .collect();
        let mut packet = Self {
            schema_version: "zyal.superreasoning.packet.v1".to_string(),
            run_id: run_id.to_string(),
            objective: objective.to_string(),
            source_runbook_sha256: source_runbook_sha256.clone(),
            effective_generations: budget_contract.effective_generations,
            budget_contract,
            model_route_contract: model_policy,
            lane_plan,
            artifact_contract,
            privacy_contract: SuperReasoningPrivacyContract {
                store_raw_reasoning: false,
                users_only_credentials: config.credential_policy
                    == CredentialSourcePolicy::UsersOnly,
                model_visible_target_values: false,
                storage_safe_summaries_only: true,
            },
            promotion_gates: SuperReasoningPromotionGates {
                proof_gate: true,
                replay_gate: config.require_replay_gate,
                parity_gate: config.parity_fail_on_required,
                leak_gate: true,
                jankurai_gate: true,
            },
            credential_policy: config.credential_policy,
            policy_hash: String::new(),
            replay_receipt: ReplayReceipt::pending(run_id, None, None, Some(source_runbook_sha256)),
            stable_hash: String::new(),
        };
        packet.policy_hash = packet.compute_policy_hash();
        packet.stable_hash = packet.policy_hash.clone();
        packet.replay_receipt.packet_hash = Some(packet.stable_hash.clone());
        packet.replay_receipt.policy_hash = Some(packet.policy_hash.clone());
        packet
    }

    /// Recompute the storage-safe stable hash.
    pub fn compute_hash(&self) -> String {
        self.compute_policy_hash()
    }

    /// Recompute the immutable policy hash. Final replay receipts and artifact
    /// hashes are intentionally excluded.
    pub fn compute_policy_hash(&self) -> String {
        sha256_json(
            &json!({
                "schema_version": self.schema_version,
                "run_id": self.run_id,
                "objective": self.objective,
                "source_runbook_sha256": self.source_runbook_sha256,
                "effective_generations": self.effective_generations,
                "budget_contract": self.budget_contract,
                "model_route_contract": self.model_route_contract,
                "lane_plan": self.lane_plan,
                "artifact_contract": self.artifact_contract,
                "privacy_contract": self.privacy_contract,
                "promotion_gates": self.promotion_gates,
                "credential_policy": self.credential_policy,
            }),
            "superreasoning_policy",
        )
    }

    /// Validate safety invariants.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != "zyal.superreasoning.packet.v1" {
            return Err(anyhow!("unsupported superreasoning packet schema"));
        }
        if self
            .lane_plan
            .iter()
            .any(|lane| lane.max_workers > MAX_SUPERREASONING_WORKERS)
        {
            return Err(anyhow!("superreasoning worker cap exceeds 10"));
        }
        let mut lane_ids = BTreeSet::new();
        for lane in &self.lane_plan {
            if lane.id.trim().is_empty() {
                return Err(anyhow!("superreasoning lane id is empty"));
            }
            if !lane_ids.insert(lane.id.as_str()) {
                return Err(anyhow!("duplicate superreasoning lane id {}", lane.id));
            }
        }
        if self.privacy_contract.store_raw_reasoning {
            return Err(anyhow!("superreasoning packet stores raw reasoning"));
        }
        if !self.privacy_contract.users_only_credentials {
            return Err(anyhow!(
                "superreasoning packet requires users-only credentials"
            ));
        }
        if self.artifact_contract.negative_memory.trim().is_empty() {
            return Err(anyhow!("superreasoning packet requires negative memory"));
        }
        if self
            .artifact_contract
            .unsupported_claims_ledger
            .trim()
            .is_empty()
        {
            return Err(anyhow!(
                "superreasoning packet requires unsupported-claims ledger"
            ));
        }
        if self.policy_hash != self.compute_policy_hash() {
            return Err(anyhow!("superreasoning packet policy hash mismatch"));
        }
        if self.stable_hash != self.compute_policy_hash() {
            return Err(anyhow!("superreasoning packet stable hash mismatch"));
        }
        Ok(())
    }

    /// Build a final replay receipt linked to this packet.
    pub fn replay_receipt(
        &self,
        artifact_hashes: Vec<SuperReasoningArtifactReceipt>,
        gate_results: SuperReasoningGateResults,
    ) -> ReplayReceipt {
        ReplayReceipt::from_gate_results(
            &self.run_id,
            self.stable_hash.clone(),
            self.policy_hash.clone(),
            self.source_runbook_sha256.clone(),
            artifact_hashes,
            gate_results,
        )
    }

    /// Artifact-only reconstruction: load the persisted packet JSON, re-derive
    /// the policy hash, and confirm the stored fields still match. Returns the
    /// reconstructed packet so callers can compare it against an expected
    /// in-memory packet.
    pub fn reconstruct_from_artifact(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("read superreasoning packet {}", path.display()))?;
        let packet: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse superreasoning packet {}", path.display()))?;
        packet.validate()?;
        Ok(packet)
    }
}
