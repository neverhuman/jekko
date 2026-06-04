use cogcore::core::RecallData;
use serde::{Deserialize, Serialize};

/// Per-recall audit record for recall explainability and the reinforcement
/// loop. The host persists this on each recall and later reports whether cited
/// ids were used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallAudit {
    /// `RecallData::context_pack_hash` identifies the exact recalled pack.
    pub pack_hash: String,
    /// The cell ids cited in the recall answer.
    pub used_ids: Vec<String>,
    /// The recall confidence.
    pub confidence: f32,
}

impl RecallAudit {
    /// Build an audit record from a cogcore recall result.
    pub fn from_recall(data: &RecallData) -> Self {
        Self {
            pack_hash: data.context_pack_hash.clone(),
            used_ids: data.used_ids.clone(),
            confidence: data.confidence,
        }
    }
}
