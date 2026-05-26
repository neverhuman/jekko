use serde_json::Value;

use super::checks::{check_live_credential_fields, check_live_provider};
use super::LiveAuditReport;

pub(crate) fn audit_model_receipts(
    packet: Option<&Value>,
    receipts: &[Value],
    report: &mut LiveAuditReport,
) {
    report.model_receipt_count = receipts.len();
    if receipts.is_empty() {
        report
            .failures
            .push("model_receipts.jsonl has no model receipts".to_string());
        return;
    }

    let budget = packet
        .and_then(|packet| packet.pointer("/budget_contract/model_call_budget"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if budget != 0 && receipts.len() as u64 > budget {
        report.failures.push(format!(
            "model receipt count {} exceeds model_call_budget {budget}",
            receipts.len()
        ));
    }

    for (idx, receipt) in receipts.iter().enumerate() {
        audit_model_receipt(idx, receipt, budget, report);
    }
}

fn audit_model_receipt(idx: usize, receipt: &Value, budget: u64, report: &mut LiveAuditReport) {
    if receipt.get("schema_version").and_then(Value::as_str) != Some("zyal.model_receipt.v1") {
        report
            .failures
            .push(format!("model_receipt[{idx}] has wrong schema_version"));
    }
    check_live_provider(receipt, &format!("model_receipt[{idx}]"), report);
    check_live_credential_fields(receipt, &format!("model_receipt[{idx}]"), report);
    if receipt.get("success").and_then(Value::as_bool) != Some(true)
        || receipt.get("status").and_then(Value::as_str) != Some("success")
    {
        report
            .failures
            .push(format!("model_receipt[{idx}] was not successful"));
    }
    if receipt
        .get("response_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        == 0
    {
        report.failures.push(format!(
            "model_receipt[{idx}] missing nonzero response_bytes"
        ));
    }
    if receipt
        .get("response_sha256")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        report
            .failures
            .push(format!("model_receipt[{idx}] missing response_sha256"));
    }
    audit_model_receipt_accounting(idx, receipt, budget, report);
}

fn audit_model_receipt_accounting(
    idx: usize,
    receipt: &Value,
    budget: u64,
    report: &mut LiveAuditReport,
) {
    if receipt.get("latency_ms").and_then(Value::as_i64).is_none() {
        report
            .failures
            .push(format!("model_receipt[{idx}] missing latency_ms"));
    }
    if receipt.get("retry_count").and_then(Value::as_u64).is_none() {
        report
            .failures
            .push(format!("model_receipt[{idx}] missing retry_count"));
    }
    if let Some(error) = receipt.get("error").and_then(Value::as_str) {
        if !error.trim().is_empty() {
            report
                .failures
                .push(format!("model_receipt[{idx}] has error: {error}"));
        }
    }
    if let Some(used) = receipt.get("budget_used").and_then(Value::as_u64) {
        if budget != 0 && used > budget {
            report.failures.push(format!(
                "model_receipt[{idx}] budget_used {used} exceeds model_call_budget {budget}"
            ));
        }
    } else {
        report
            .failures
            .push(format!("model_receipt[{idx}] missing budget_used"));
    }
    if receipt
        .get("budget_remaining")
        .and_then(Value::as_u64)
        .is_none()
    {
        report
            .failures
            .push(format!("model_receipt[{idx}] missing budget_remaining"));
    }
}
