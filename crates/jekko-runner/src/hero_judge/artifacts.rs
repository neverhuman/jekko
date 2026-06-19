use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::superreasoning::SuperReasoningPacket;

use super::{HeroJudgeQualityMetric, HeroJudgeReviewCard, PromotionDecision};

/// Prompt lineage row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptVariant {
    /// Prompt id.
    pub id: String,
    /// `hero` or `judge`.
    pub role: String,
    /// Generation.
    pub generation: usize,
    /// Optional parent prompt id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Storage-safe summary, never raw chain-of-thought.
    pub summary: String,
    /// Prompt hash.
    pub prompt_sha256: String,
    /// Deterministic host score.
    pub score: f64,
    /// Variant status.
    pub status: String,
}

/// One lane artifact row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeLaneArtifact {
    /// Artifact id.
    pub id: String,
    /// Generation.
    pub generation: usize,
    /// Lane kind.
    pub kind: String,
    /// Lane index.
    pub lane: usize,
    /// Model receipt id.
    pub model_receipt_id: String,
    /// Storage-safe summary.
    pub summary: String,
    /// Content hash.
    pub content_sha256: String,
    /// Deterministic score.
    pub score: f64,
    /// Plot-ready host-side lane metrics.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, f64>,
    /// Status.
    pub status: String,
}

/// Reviewer packet for checking progress without raw chain-of-thought.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeReviewerPacket {
    /// Run id.
    pub run_id: String,
    /// Objective text used by the run.
    pub objective: String,
    /// Human-review guidance.
    pub reviewer_questions: Vec<String>,
    /// Per-generation quality metrics.
    pub quality_metrics: Vec<HeroJudgeQualityMetric>,
    /// Last promotion decision.
    pub promotion_decision: PromotionDecision,
    /// Storage-safe cards separated by role group.
    pub cards: Vec<HeroJudgeReviewCard>,
    /// Superreasoning packet path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superreasoning_packet_path: Option<PathBuf>,
    /// Superreasoning packet stable hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superreasoning_packet_hash: Option<String>,
    /// Full superreasoning packet embedded inline for offline reviewers
    /// (spec v2 §"Embed the full packet in `reviewer_packet.json`").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superreasoning_packet: Option<SuperReasoningPacket>,
    /// Replay receipt path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_receipt_path: Option<PathBuf>,
    /// Proof gate status.
    #[serde(default)]
    pub proof_gate_passed: bool,
    /// Replay gate status.
    #[serde(default)]
    pub replay_gate_passed: bool,
    /// Parity gate status.
    #[serde(default)]
    pub parity_gate_passed: bool,
    /// Leak gate status.
    #[serde(default)]
    pub leak_gate_passed: bool,
    /// Jankurai gate status.
    #[serde(default)]
    pub jankurai_gate_passed: bool,
    /// Unsupported claims ledger path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_claims_jsonl: Option<PathBuf>,
    /// Negative memory ledger path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_memory_jsonl: Option<PathBuf>,
}
