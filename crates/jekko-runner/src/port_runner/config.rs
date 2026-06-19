use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model_client::ModelCallReceipt;
use crate::port::{PortMasterPlan, PortRuntimeOptions, PortTargetRequest};
use crate::reasoning::AdvancedReasoningConfig;
use crate::reasoning_runner::AdvancedReasoningSummary;

/// Config accepted by `jekko-runner port-run --config`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortRunConfig {
    /// Target request.
    #[serde(flatten)]
    pub target: PortTargetRequest,
    /// Whether fake worker receipts should be emitted.
    #[serde(default = "default_fake_worker")]
    pub fake_worker_cycle: bool,
    /// Whether a dirty tree is allowed.
    #[serde(default)]
    pub allow_dirty: bool,
    /// Advanced reasoning runtime config.
    #[serde(default)]
    pub advanced_reasoning: AdvancedReasoningConfig,
    /// Runtime proof options.
    #[serde(flatten)]
    pub runtime: PortRuntimeOptions,
}

/// One durable port tick report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortTickReport {
    /// Run id.
    pub run_id: String,
    /// Target id.
    pub target_id: String,
    /// Draft plan.
    pub plan: PortMasterPlan,
    /// Model receipt.
    pub model_receipt: ModelCallReceipt,
    /// Graph summary by kind.
    pub graph_summary: serde_json::Value,
    /// Fake task completed, if any.
    pub fake_task_completed: Option<String>,
    /// Advanced reasoning summary, when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced_reasoning: Option<AdvancedReasoningSummary>,
}

/// Parse a JSON or TOML port config.
pub fn read_port_run_config(path: &Path) -> Result<PortRunConfig> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&text).context("parse JSON port run config"),
        Some("toml") => toml::from_str(&text).context("parse TOML port run config"),
        Some(ext) => {
            anyhow::bail!("unsupported port run config extension .{ext}; use .json or .toml")
        }
        None => anyhow::bail!("port run config path must end in .json or .toml"),
    }
}

fn default_fake_worker() -> bool {
    true
}
