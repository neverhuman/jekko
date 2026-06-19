//! Integration tests for the run_summary builder.

use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::{build, render_markdown, write_summary};

fn write_events(dir: &std::path::Path, events: &[serde_json::Value]) {
    let path = dir.join("events.jsonl");
    let mut body = String::new();
    for ev in events {
        body.push_str(&serde_json::to_string(ev).unwrap());
        body.push('\n');
    }
    fs::write(path, body).unwrap();
}

#[test]
fn builds_minimal_summary_with_no_events_file() {
    let dir = tempdir().unwrap();
    let summary = build(dir.path()).unwrap();
    assert_eq!(summary.schema_version, "zyal.run_summary.v1");
    assert_eq!(summary.terminal_status, "halted");
    assert!(summary
        .operator_next_steps
        .iter()
        .any(|s| s.contains("No events.jsonl")));
}

#[test]
fn detects_run_finished_terminal_status() {
    let dir = tempdir().unwrap();
    write_events(
        dir.path(),
        &[
            json!({"ts": 1, "kind": "run_started", "data": {"workflow": "zyal_hero_judge"}}),
            json!({"ts": 5, "kind": "model_attempt", "data": {"kind": "frame", "attempt": 1}}),
            json!({"ts": 6, "kind": "model_attempt_outcome", "data": {"state": "parsed", "response_bytes": 200, "credential_user_id": "user_1", "provider": "groq", "latency_ms": 800}}),
            json!({"ts": 8, "kind": "run_finished", "data": {"status": "complete"}}),
        ],
    );
    let summary = build(dir.path()).unwrap();
    assert_eq!(summary.terminal_status, "run_finished");
    assert_eq!(summary.pipeline, "zyal_hero_judge");
    assert_eq!(summary.model_calls.total_attempts, 1);
    assert_eq!(summary.model_calls.parsed_outcomes, 1);
    assert_eq!(summary.duration_seconds, Some(7));
}

#[test]
fn detects_empty_response_streak_halt() {
    let dir = tempdir().unwrap();
    write_events(
        dir.path(),
        &[
            json!({"ts": 1, "kind": "run_started", "data": {"workflow": "zyal_advanced_port"}}),
            json!({"ts": 2, "kind": "reasoning_state", "data": {"state": "stage_brainstorm"}}),
            json!({"ts": 3, "kind": "model_attempt", "data": {"kind": "stage_brainstorm", "attempt": 1}}),
            json!({"ts": 4, "kind": "model_attempt_outcome", "data": {"state": "retryable_failure", "response_bytes": 0, "credential_user_id": "user_1", "provider": "jnoccio"}}),
            json!({"ts": 5, "kind": "model_attempt", "data": {"kind": "stage_brainstorm", "attempt": 2}}),
            json!({"ts": 6, "kind": "model_attempt_outcome", "data": {"state": "retryable_failure", "response_bytes": 0, "credential_user_id": "user_2", "provider": "jnoccio"}}),
            json!({"ts": 7, "kind": "model_attempt", "data": {"kind": "stage_brainstorm", "attempt": 3}}),
            json!({"ts": 8, "kind": "model_attempt_outcome", "data": {"state": "final_block", "response_bytes": 0, "credential_user_id": "user_1", "provider": "jnoccio"}}),
            json!({"ts": 9, "kind": "empty_response_streak", "data": {"kind": "stage_brainstorm", "count": 3, "providers_tried": ["jnoccio"], "users_tried": ["user_1", "user_2"]}}),
        ],
    );
    let summary = build(dir.path()).unwrap();
    assert_ne!(summary.terminal_status, "run_finished");
    let halt = summary.halt_reason.as_ref().expect("halt_reason populated");
    assert_eq!(halt.kind, "empty_response_streak");
    assert_eq!(halt.stage.as_deref(), Some("stage_brainstorm"));
    assert!(halt.summary.contains("quality_band"));
    // Operator next-step should suggest quality_band.
    assert!(summary
        .operator_next_steps
        .iter()
        .any(|s| s.contains("quality_band")));
}

#[test]
fn folds_budget_max_calls_and_quality_bands() {
    let dir = tempdir().unwrap();
    write_events(
        dir.path(),
        &[
            json!({"ts": 1, "kind": "run_started", "data": {"workflow": "zyal_advanced_port"}}),
            json!({"ts": 2, "kind": "model_attempt", "data": {"kind": "stage_brainstorm", "attempt": 1}}),
            json!({"ts": 3, "kind": "live_budget", "data": {"used": 4, "remaining": 60}}),
            json!({"ts": 4, "kind": "model_attempt_outcome", "data": {"state": "parsed", "response_bytes": 200, "quality_band": "top20", "credential_user_id": "user_1", "provider": "jnoccio", "latency_ms": 1000}}),
        ],
    );
    let summary = build(dir.path()).unwrap();
    assert_eq!(summary.budget.max_calls, Some(64));
    assert_eq!(
        summary.model_calls.by_quality_band.get("top20").copied(),
        Some(1)
    );
}

#[test]
fn parity_gap_populates_terminal_halt_reason() {
    let dir = tempdir().unwrap();
    write_events(
        dir.path(),
        &[
            json!({"ts": 1, "kind": "run_started", "data": {"workflow": "zyal_advanced_port"}}),
            json!({"ts": 2, "kind": "reasoning_state", "data": {"state": "close_parity_perf"}}),
            json!({"ts": 3, "kind": "parity_gap", "data": {"count": 2}}),
        ],
    );
    let summary = build(dir.path()).unwrap();
    let halt = summary.halt_reason.as_ref().expect("halt_reason");
    assert_eq!(halt.kind, "parity_gap");
}

#[test]
fn round_trips_through_disk() {
    let dir = tempdir().unwrap();
    write_events(
        dir.path(),
        &[
            json!({"ts": 1, "kind": "run_started", "data": {"workflow": "zyal_hero_judge"}}),
            json!({"ts": 5, "kind": "run_finished", "data": {"status": "complete"}}),
        ],
    );
    let summary = build(dir.path()).unwrap();
    write_summary(dir.path(), &summary).unwrap();
    let json = fs::read_to_string(dir.path().join("summary.json")).unwrap();
    let parsed: super::types::RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.schema_version, "zyal.run_summary.v1");
    assert_eq!(parsed.terminal_status, "run_finished");
    let md = fs::read_to_string(dir.path().join("summary.md")).unwrap();
    assert!(md.contains("# Run summary"));
    assert!(md.contains("Terminal status"));
}

#[test]
fn markdown_includes_halt_callout_when_present() {
    let mut summary = super::types::RunSummary::empty("test");
    summary.halt_reason = Some(super::types::HaltReason {
        kind: "empty_response_streak".to_string(),
        stage: Some("stage_brainstorm".to_string()),
        consecutive_attempts: Some(3),
        providers_tried: vec!["jnoccio".to_string()],
        users_tried: vec!["user_1".to_string(), "user_2".to_string()],
        summary: "test halt summary".to_string(),
    });
    let md = render_markdown(&summary);
    assert!(md.contains("Halt reason"));
    assert!(md.contains("empty_response_streak"));
    assert!(md.contains("stage_brainstorm"));
}
