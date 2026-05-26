use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::model_policy::ModelTaskKind;

use super::labels::{kind_label, receipt_id};

/// Credential source policy forwarded to live runtime child processes.
///
/// Canonical definition lives in `zyal-core`; re-exported here so existing
/// `crate::model_client::CredentialSourcePolicy` paths keep compiling.
pub use zyal_core::CredentialSourcePolicy;

/// Receipt emitted for every model call attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCallReceipt {
    /// Stable-ish receipt id.
    pub id: String,
    /// Model task kind.
    pub kind: String,
    /// Optional task id this call served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Provider id, or `fake`.
    pub provider: String,
    /// Model id.
    pub model: String,
    /// Latency in milliseconds.
    pub latency_ms: u64,
    /// Success flag.
    pub success: bool,
    /// Optional cost when reported by the provider layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Assistant text or fake response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    /// Error text when the call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Calls consumed in the active live budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_used: Option<usize>,
    /// Calls remaining in the active live budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_remaining: Option<usize>,
    /// Route label used by the workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Credential source policy used by a live child process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_policy: Option<String>,
    /// User folder id selected before router metadata confirms the winner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_credential_user_id: Option<String>,
    /// User folder id for users-only credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_user_id: Option<String>,
    /// Retry count reported by the runtime, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<usize>,
}

impl ModelCallReceipt {
    /// Deterministic success constructor for tests.
    pub fn fake_success(kind: ModelTaskKind, response: impl Into<String>) -> Self {
        Self {
            id: receipt_id("fake"),
            kind: kind_label(kind).to_string(),
            task_id: None,
            provider: "fake".to_string(),
            model: "fake-model".to_string(),
            latency_ms: 0,
            success: true,
            cost_usd: Some(0.0),
            response: Some(response.into()),
            error: None,
            budget_used: None,
            budget_remaining: None,
            route: Some(kind_label(kind).to_string()),
            credential_policy: None,
            selected_credential_user_id: None,
            credential_user_id: None,
            retry_count: Some(0),
        }
    }

    /// Deterministic failure constructor.
    pub fn failure(
        kind: ModelTaskKind,
        provider: impl Into<String>,
        model: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            id: receipt_id("failure"),
            kind: kind_label(kind).to_string(),
            task_id: None,
            provider: provider.into(),
            model: model.into(),
            latency_ms: 0,
            success: false,
            cost_usd: None,
            response: None,
            error: Some(error.into()),
            budget_used: None,
            budget_remaining: None,
            route: Some(kind_label(kind).to_string()),
            credential_policy: None,
            selected_credential_user_id: None,
            credential_user_id: None,
            retry_count: Some(0),
        }
    }
}

/// Model completion boundary.
#[async_trait]
pub trait ModelClient: Send + Sync {
    /// Complete a planning prompt.
    async fn complete(
        &self,
        kind: ModelTaskKind,
        prompt: &str,
        cwd: &Path,
    ) -> Result<ModelCallReceipt>;
}
