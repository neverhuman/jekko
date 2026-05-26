use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LiveAuditReport {
    pub schema_version: String,
    pub run_dir: PathBuf,
    pub strict: bool,
    pub replay_status: Option<String>,
    pub artifact_count: usize,
    pub model_receipt_count: usize,
    pub model_outcome_event_count: usize,
    pub status: String,
    pub failures: Vec<String>,
    pub warnings: Vec<String>,
}

impl LiveAuditReport {
    pub fn exit_code(&self) -> i32 {
        if self.status == "passed" {
            0
        } else {
            1
        }
    }
}
