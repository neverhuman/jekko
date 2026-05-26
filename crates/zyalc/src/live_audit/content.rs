use std::fs;
use std::path::Path;

use serde_json::Value;

use super::files::non_empty_jsonl_line_count;
use super::{LiveAuditReport, EVENTS_FILE, FORBIDDEN_CREDENTIAL_MARKERS, REQUIRED_ARTIFACTS};

pub(crate) fn audit_ledgers(run_dir: &Path, report: &mut LiveAuditReport) {
    for ledger in [
        "claim_ledger.jsonl",
        "unsupported_claims.jsonl",
        "negative_memory.jsonl",
    ] {
        let path = run_dir.join(ledger);
        if path.exists() && non_empty_jsonl_line_count(&path, report) == 0 {
            report
                .failures
                .push(format!("required ledger {ledger} is empty"));
        }
    }
}

pub(crate) fn audit_forbidden_content(
    run_dir: &Path,
    packet: Option<&Value>,
    report: &mut LiveAuditReport,
) {
    let mut forbidden = packet
        .and_then(|packet| packet.pointer("/artifact_contract/forbidden_content"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    forbidden.extend(
        FORBIDDEN_CREDENTIAL_MARKERS
            .iter()
            .map(|value| value.to_string()),
    );

    for file in REQUIRED_ARTIFACTS.iter().copied().chain([EVENTS_FILE]) {
        audit_forbidden_file(run_dir, file, &forbidden, report);
    }
}

fn audit_forbidden_file(
    run_dir: &Path,
    file: &str,
    forbidden: &[String],
    report: &mut LiveAuditReport,
) {
    let path = run_dir.join(file);
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let lower = text.to_ascii_lowercase();
    let markers = if matches!(file, "superreasoning_packet.json" | "reviewer_packet.json") {
        FORBIDDEN_CREDENTIAL_MARKERS
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
    } else {
        forbidden.to_vec()
    };
    for marker in &markers {
        if lower.contains(&marker.to_ascii_lowercase()) {
            report.failures.push(format!(
                "forbidden marker {marker:?} found in {}",
                path.display()
            ));
        }
    }
}
