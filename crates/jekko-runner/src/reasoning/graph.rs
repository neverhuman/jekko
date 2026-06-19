use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ReasoningRole;

/// Edge between artifacts in the reasoning graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningEdge {
    /// Owning run id.
    pub run_id: String,
    /// Source artifact id.
    pub src_artifact_id: String,
    /// Destination artifact id.
    pub dst_artifact_id: String,
    /// Edge kind.
    pub kind: String,
    /// Optional weight.
    pub weight: Option<f64>,
    /// Structured payload.
    #[serde(default)]
    pub payload_json: Value,
}

impl ReasoningEdge {
    /// Validate graph edge invariants.
    pub fn validate(&self) -> Result<()> {
        if self.src_artifact_id == self.dst_artifact_id {
            return Err(anyhow!("reasoning edge cannot point to itself"));
        }
        if self.kind.trim().is_empty() {
            return Err(anyhow!("reasoning edge kind cannot be empty"));
        }
        if let Some(weight) = self.weight {
            if !weight.is_finite() {
                return Err(anyhow!("reasoning edge weight must be finite"));
            }
        }
        Ok(())
    }
}

/// One independent reasoning lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningLane {
    /// Lane id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Lane role.
    pub role: ReasoningRole,
    /// Diversity strategy.
    pub strategy: String,
    /// Lane status.
    pub status: String,
    /// Artifacts produced by this lane.
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    /// Declared write scope.
    #[serde(default)]
    pub write_scope: Vec<String>,
    /// Worker id if assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// Lane confidence after reduction.
    pub confidence: f64,
}

/// Reasoning tournament metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningTournament {
    /// Tournament id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Objective.
    pub objective: String,
    /// Lane ids.
    #[serde(default)]
    pub lane_ids: Vec<String>,
    /// Reducer artifact id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reducer_artifact_id: Option<String>,
    /// Status.
    pub status: String,
}
