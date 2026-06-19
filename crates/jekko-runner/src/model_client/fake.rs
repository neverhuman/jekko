use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use crate::model_policy::ModelTaskKind;

use super::labels::{kind_label, receipt_id};
use super::{ModelCallReceipt, ModelClient};

/// Fake deterministic model client for CI.
#[derive(Debug, Clone)]
pub struct FakeModelClient {
    response: String,
    fail: bool,
    delay: Option<Duration>,
}

impl FakeModelClient {
    /// Build a successful fake client.
    pub fn success(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            fail: false,
            delay: None,
        }
    }

    /// Build a failing fake client.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            response: error.into(),
            fail: true,
            delay: None,
        }
    }

    /// Inject an artificial per-call delay. Used by the parallel-brainstorm
    /// wall-time test to verify that lanes actually progress concurrently.
    pub fn with_delay(mut self, ms: u64) -> Self {
        self.delay = Some(Duration::from_millis(ms));
        self
    }
}

#[async_trait]
impl ModelClient for FakeModelClient {
    async fn complete(
        &self,
        kind: ModelTaskKind,
        _prompt: &str,
        _cwd: &Path,
    ) -> Result<ModelCallReceipt> {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        if self.fail {
            Ok(ModelCallReceipt {
                id: receipt_id("fake"),
                kind: kind_label(kind).to_string(),
                task_id: None,
                provider: "fake".to_string(),
                model: "fake-model".to_string(),
                latency_ms: 0,
                success: false,
                cost_usd: Some(0.0),
                response: None,
                error: Some(self.response.clone()),
                budget_used: None,
                budget_remaining: None,
                route: Some(kind_label(kind).to_string()),
                credential_policy: None,
                selected_credential_user_id: None,
                credential_user_id: None,
                retry_count: Some(0),
                quality_band: None,
            })
        } else {
            Ok(ModelCallReceipt::fake_success(kind, self.response.clone()))
        }
    }
}
