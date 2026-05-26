use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{LiveAuditReport, EVENTS_FILE, REQUIRED_ARTIFACTS};

pub(crate) fn audit_required_files(run_dir: &Path, report: &mut LiveAuditReport) {
    for file in REQUIRED_ARTIFACTS.iter().copied().chain([EVENTS_FILE]) {
        if file == "superreasoning_packet.json" {
            continue;
        }
        let path = run_dir.join(file);
        if !path.exists() {
            report.failures.push(format!(
                "required live audit file missing: {}",
                path.display()
            ));
        }
    }
}

pub(crate) fn load_json_optional(path: &Path, report: &mut LiveAuditReport) -> Option<Value> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                report
                    .failures
                    .push(format!("parse {}: {err}", path.display()));
                None
            }
        },
        Err(err) => {
            report
                .failures
                .push(format!("read {}: {err}", path.display()));
            None
        }
    }
}

pub(crate) fn load_jsonl_optional(path: &Path, report: &mut LiveAuditReport) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        report
            .failures
            .push(format!("read {} failed", path.display()));
        return Vec::new();
    };
    let mut values = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(value) => values.push(value),
            Err(err) => report.failures.push(format!(
                "parse JSONL {} line {}: {err}",
                path.display(),
                idx + 1
            )),
        }
    }
    values
}

pub(crate) fn non_empty_jsonl_line_count(path: &Path, report: &mut LiveAuditReport) -> usize {
    match fs::read_to_string(path) {
        Ok(text) => text.lines().filter(|line| !line.trim().is_empty()).count(),
        Err(err) => {
            report
                .failures
                .push(format!("read ledger {}: {err}", path.display()));
            0
        }
    }
}
