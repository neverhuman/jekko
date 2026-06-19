use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hashing::sha256_json;

use super::{AdvancedReasoningConfig, EvidenceLevel, ReasoningArtifactKind, ReasoningRole};

/// One structured reasoning artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningArtifact {
    /// Artifact id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Producer role.
    pub role: ReasoningRole,
    /// Artifact kind.
    pub kind: ReasoningArtifactKind,
    /// Short title.
    pub title: String,
    /// Stored summary, not chain-of-thought.
    pub summary: String,
    /// Structured payload.
    #[serde(default)]
    pub payload_json: Value,
    /// Evidence strength.
    pub evidence_level: EvidenceLevel,
    /// Calibrated confidence.
    pub confidence: f64,
    /// Source artifact ids.
    #[serde(default)]
    pub source_artifact_ids: Vec<String>,
    /// Verification receipt ids.
    #[serde(default)]
    pub verifier_receipt_ids: Vec<String>,
    /// Raw model reasoning. Redacted before storage unless explicitly allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_reasoning: Option<String>,
    /// Stable SHA-256 over the storage-safe content.
    pub content_hash: String,
    /// Artifact status.
    pub status: String,
}

impl ReasoningArtifact {
    /// Construct a storage-safe artifact and apply confidence/redaction rules.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        run_id: impl Into<String>,
        role: ReasoningRole,
        kind: ReasoningArtifactKind,
        title: impl Into<String>,
        summary: impl Into<String>,
        evidence_level: EvidenceLevel,
        confidence: f64,
        payload_json: Value,
    ) -> Self {
        let mut artifact = Self {
            id: id.into(),
            run_id: run_id.into(),
            role,
            kind,
            title: title.into(),
            summary: summary.into(),
            payload_json,
            evidence_level,
            confidence,
            source_artifact_ids: Vec::new(),
            verifier_receipt_ids: Vec::new(),
            raw_reasoning: None,
            content_hash: String::new(),
            status: "candidate".to_string(),
        };
        artifact.refresh_hash();
        artifact
    }

    /// Apply config policy before durable storage.
    pub fn prepare_for_storage(&mut self, config: &AdvancedReasoningConfig) {
        if !config.store_raw_reasoning {
            self.raw_reasoning = None;
        }
        if !self.evidence_level.has_executable_evidence() {
            self.confidence = self
                .confidence
                .min(config.effective_confidence_cap())
                .max(0.0);
        } else if self.confidence.is_finite() {
            self.confidence = self.confidence.clamp(0.0, 1.0);
        } else {
            self.confidence = 0.0;
        }
        self.refresh_hash();
    }

    /// Recompute the storage-safe hash.
    pub fn refresh_hash(&mut self) {
        self.content_hash = stable_reasoning_hash(&ReasoningHashPayload {
            id: &self.id,
            run_id: &self.run_id,
            role: self.role,
            kind: self.kind,
            title: &self.title,
            summary: &self.summary,
            payload_json: &self.payload_json,
            evidence_level: self.evidence_level,
            confidence: self.confidence,
            source_artifact_ids: &self.source_artifact_ids,
            verifier_receipt_ids: &self.verifier_receipt_ids,
            status: &self.status,
        });
    }
}

#[derive(Serialize)]
struct ReasoningHashPayload<'a> {
    id: &'a str,
    run_id: &'a str,
    role: ReasoningRole,
    kind: ReasoningArtifactKind,
    title: &'a str,
    summary: &'a str,
    payload_json: &'a Value,
    evidence_level: EvidenceLevel,
    confidence: f64,
    source_artifact_ids: &'a [String],
    verifier_receipt_ids: &'a [String],
    status: &'a str,
}

/// Stable SHA-256 over a serializable payload.
pub fn stable_reasoning_hash<T: Serialize>(value: &T) -> String {
    sha256_json(value, "reasoning_hash")
}
