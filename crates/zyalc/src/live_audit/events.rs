use serde_json::Value;

use super::checks::{check_live_credential_fields, check_live_provider};
use super::{LiveAuditReport, FORBIDDEN_OUTCOME_STATES};

pub(crate) fn audit_events(events: &[Value], report: &mut LiveAuditReport) {
    let run_started = events.iter().find(|event| event["kind"] == "run_started");
    if let Some(event) = run_started {
        let data = &event["data"];
        if data.get("live_model_calls").and_then(Value::as_bool) != Some(true) {
            report
                .failures
                .push("run_started must record live_model_calls=true".to_string());
        }
        if data.get("credential_policy").and_then(Value::as_str) != Some("users-only") {
            report
                .failures
                .push("run_started must record credential_policy=users-only".to_string());
        }
        if data.get("mock_llm_set").and_then(Value::as_bool) != Some(false) {
            report
                .failures
                .push("run_started must record mock_llm_set=false".to_string());
        }
    } else {
        report
            .failures
            .push("events.jsonl missing run_started event".to_string());
    }

    for (idx, event) in events
        .iter()
        .filter(|event| event["kind"] == "model_outcome")
        .enumerate()
    {
        audit_model_outcome(idx, &event["data"], report);
    }

    if report.model_outcome_event_count == 0 {
        report
            .failures
            .push("events.jsonl has no model_outcome events".to_string());
    }
}

fn audit_model_outcome(idx: usize, data: &Value, report: &mut LiveAuditReport) {
    report.model_outcome_event_count += 1;
    let state = data.get("state").and_then(Value::as_str).unwrap_or("");
    if FORBIDDEN_OUTCOME_STATES.contains(&state) {
        report
            .failures
            .push(format!("model_outcome[{idx}] has forbidden state {state}"));
    }
    if state != "parsed" {
        report.failures.push(format!(
            "model_outcome[{idx}] state is {state}, expected parsed"
        ));
    }
    check_live_provider(data, &format!("model_outcome[{idx}]"), report);
    check_live_credential_fields(data, &format!("model_outcome[{idx}]"), report);
    if data.get("success").and_then(Value::as_bool) != Some(true) {
        report
            .failures
            .push(format!("model_outcome[{idx}] was not successful"));
    }
    if data
        .get("response_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        == 0
    {
        report.failures.push(format!(
            "model_outcome[{idx}] missing nonzero response_bytes"
        ));
    }
    if data.get("latency_ms").and_then(Value::as_u64).is_none() {
        report
            .failures
            .push(format!("model_outcome[{idx}] missing latency_ms"));
    }
    if data.get("retry_count").and_then(Value::as_u64).is_none() {
        report
            .failures
            .push(format!("model_outcome[{idx}] missing retry_count"));
    }
}
