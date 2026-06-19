//! Storage-safe parity report types shared across cases, results, and gaps.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One target-switched parity case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityCase {
    /// Stable case id.
    pub id: String,
    /// Case tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Adapter kind.
    pub target_kind: String,
    /// Case steps.
    #[serde(default)]
    pub steps: Vec<ParityStep>,
    /// Performance budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf: Option<ParityPerfBudget>,
}

impl ParityCase {
    /// Whether this case is required by the approval gate.
    pub fn is_required(&self) -> bool {
        self.tags
            .iter()
            .any(|tag| tag == "required" || tag == "approved")
    }

    /// Whether this case requires performance data.
    pub fn requires_perf(&self) -> bool {
        self.perf.is_some() || self.tags.iter().any(|tag| tag == "perf")
    }
}

/// One protocol or command step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityStep {
    /// Input to send.
    pub send: String,
    /// Expected output.
    pub expect: String,
}

/// Performance budget for a case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityPerfBudget {
    /// Maximum candidate/reference p95 ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_ms_max_ratio: Option<f64>,
}

/// One parity case result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParityResult {
    /// Case id.
    pub case_id: String,
    /// Target name, such as `reference` or `candidate`.
    pub target: String,
    /// Result status.
    pub status: String,
    /// Whether the case was skipped.
    #[serde(default)]
    pub skipped: bool,
    /// Optional message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Performance data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf: Option<serde_json::Value>,
    /// SHA-256 of stdout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_sha256: Option<String>,
    /// SHA-256 of stderr.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_sha256: Option<String>,
    /// Process exit code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Elapsed nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_nanos: Option<u128>,
    /// Candidate/reference latency ratio for this case, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ratio: Option<f64>,
    /// Case artifact directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_dir: Option<PathBuf>,
    /// Extra diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<serde_json::Value>,
}

/// Full parity report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityReport {
    /// Report schema version.
    pub schema_version: String,
    /// Reference target label.
    pub reference: String,
    /// Candidate target label.
    pub candidate: String,
    /// Results.
    #[serde(default)]
    pub results: Vec<ParityResult>,
}

/// RedlineDB-style parity artifact paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParityArtifacts {
    /// Generated case manifest.
    pub generated_manifest_json: PathBuf,
    /// Approved CI case id list.
    pub approved_ci_txt: PathBuf,
    /// Raw JSONL results.
    pub raw_jsonl: PathBuf,
    /// Summary JSON report.
    pub summary_json: PathBuf,
    /// Gap JSON report.
    pub gaps_json: PathBuf,
}

/// Summary written to `target/zyal/parity/<run_id>/summary.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParitySummary {
    /// Report schema version.
    pub schema_version: String,
    /// Overall status.
    pub status: String,
    /// Number of declared cases.
    pub case_count: usize,
    /// Passed result count.
    pub passed: usize,
    /// Failed result count.
    pub failed: usize,
    /// Skipped result count.
    pub skipped: usize,
    /// Required perf cases missing perf data.
    pub missing_perf: usize,
    /// Perf-budget failures.
    pub perf_over_budget: usize,
    /// Generated parity gaps.
    #[serde(default)]
    pub gaps: Vec<ParityGap>,
    /// Full report.
    pub report: ParityReport,
}

/// Redline-style generated manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedParityManifest {
    /// Manifest schema.
    pub schema_version: String,
    /// Run id.
    pub run_id: String,
    /// Number of generated cases.
    pub case_count: usize,
    /// Generated cases.
    pub cases: Vec<GeneratedParityCase>,
}

/// One generated parity case manifest row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedParityCase {
    /// Case id.
    pub id: String,
    /// Target kind.
    pub target_kind: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Whether the case is approved for CI gates.
    pub approved: bool,
    /// Step count.
    pub step_count: usize,
    /// Whether performance data is required.
    pub requires_perf: bool,
}

/// Redline-style raw row written to `raw.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawParityRow {
    /// Schema version.
    pub schema_version: String,
    /// Case id.
    pub case_id: String,
    /// Target name.
    pub target: String,
    /// Status.
    pub status: String,
    /// Whether skipped.
    pub skipped: bool,
    /// Exit code.
    pub exit_code: Option<i32>,
    /// Elapsed nanoseconds.
    pub elapsed_nanos: Option<u128>,
    /// Stdout hash.
    pub stdout_sha256: Option<String>,
    /// Stderr hash.
    pub stderr_sha256: Option<String>,
    /// Perf payload.
    pub perf: Option<serde_json::Value>,
    /// Message.
    pub message: Option<String>,
}

/// Follow-up work generated from a parity failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityGap {
    /// Gap id.
    pub id: String,
    /// Case id.
    pub case_id: String,
    /// Gap category.
    pub category: String,
    /// Profile or target lane.
    pub profile: String,
    /// Priority.
    pub priority: u8,
    /// Human-readable message.
    pub message: String,
    /// Follow-up task payload.
    pub follow_up_task: serde_json::Value,
}
