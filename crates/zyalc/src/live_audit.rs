//! Strict live-run audit for ZYAL superreasoning proof directories.
//!
//! This is intentionally independent of `jankurai-runner`: it validates the
//! run directory artifacts, replay receipt, event stream, and sanitized model
//! receipts after the producer has finished.

mod checks;
mod content;
mod events;
mod files;
mod packet;
mod receipts;
mod report;

use std::path::Path;

use anyhow::Result;

use crate::replay_verify;
use content::{audit_forbidden_content, audit_ledgers};
use events::audit_events;
use files::{audit_required_files, load_json_optional, load_jsonl_optional};
use packet::audit_packet;
use receipts::audit_model_receipts;
pub use report::LiveAuditReport;

const EVENTS_FILE: &str = "events.jsonl";
const MODEL_RECEIPTS_FILE: &str = "model_receipts.jsonl";

const REQUIRED_ARTIFACTS: &[&str] = &[
    "superreasoning_packet.json",
    "reviewer_packet.json",
    "replay_receipt.json",
    "model_receipts.jsonl",
    "claim_ledger.jsonl",
    "unsupported_claims.jsonl",
    "negative_memory.jsonl",
];

const FORBIDDEN_OUTCOME_STATES: &[&str] = &[
    "live_parse_substitution",
    "fake_provider_synthetic_response",
];

// Canonical credential-leakage list lives in zyal-core. Re-exported under
// the legacy name so existing `super::FORBIDDEN_CREDENTIAL_MARKERS` paths in
// `content.rs` keep compiling unchanged.
pub(crate) use zyal_core::FORBIDDEN_CREDENTIAL_PATTERNS as FORBIDDEN_CREDENTIAL_MARKERS;

pub fn audit(run_dir: &Path, strict: bool) -> Result<LiveAuditReport> {
    let mut report = LiveAuditReport {
        schema_version: "zyal.live_audit.v1".to_string(),
        run_dir: run_dir.to_path_buf(),
        strict,
        replay_status: None,
        artifact_count: 0,
        model_receipt_count: 0,
        model_outcome_event_count: 0,
        status: "failed".to_string(),
        failures: Vec::new(),
        warnings: Vec::new(),
    };

    audit_replay(run_dir, &mut report);
    let packet = load_json_optional(&run_dir.join("superreasoning_packet.json"), &mut report);
    audit_packet(packet.as_ref(), &mut report);
    audit_required_files(run_dir, &mut report);

    let events = load_jsonl_optional(&run_dir.join(EVENTS_FILE), &mut report);
    audit_events(&events, &mut report);

    let model_receipts = load_jsonl_optional(&run_dir.join(MODEL_RECEIPTS_FILE), &mut report);
    audit_model_receipts(packet.as_ref(), &model_receipts, &mut report);

    audit_ledgers(run_dir, &mut report);
    audit_forbidden_content(run_dir, packet.as_ref(), &mut report);

    if report.model_receipt_count != 0
        && report.model_outcome_event_count != 0
        && report.model_receipt_count != report.model_outcome_event_count
    {
        report.failures.push(format!(
            "model receipt count {} does not match model_outcome event count {}",
            report.model_receipt_count, report.model_outcome_event_count
        ));
    }

    if report.failures.is_empty() {
        report.status = "passed".to_string();
    }
    Ok(report)
}

fn audit_replay(run_dir: &Path, report: &mut LiveAuditReport) {
    match replay_verify::verify(run_dir) {
        Ok(replay) => {
            report.replay_status = Some(replay.status.clone());
            report.artifact_count = replay.artifact_count;
            if replay.status != "passed" {
                report.failures.push(format!(
                    "verify-replay status is {}, expected passed",
                    replay.status
                ));
                report.failures.extend(replay.failures);
            }
        }
        Err(err) => report
            .failures
            .push(format!("verify-replay could not run: {err:#}")),
    }
}

pub fn audit_strict(run_dir: &Path) -> Result<LiveAuditReport> {
    let report = audit(run_dir, true)?;
    if report.status != "passed" {
        anyhow::bail!("live audit failed: {}", report.failures.join("; "));
    }
    Ok(report)
}

pub fn print_report(report: &LiveAuditReport, format: &str) -> Result<()> {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else if report.status == "passed" {
        println!(
            "zyalc audit-live-run: passed - {} model receipt(s), {} model_outcome event(s)",
            report.model_receipt_count, report.model_outcome_event_count
        );
    } else {
        eprintln!("zyalc audit-live-run: failed");
        for failure in &report.failures {
            eprintln!("  - {failure}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
