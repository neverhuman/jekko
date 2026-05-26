use serde_json::Value;

use super::LiveAuditReport;

pub(crate) fn check_live_provider(value: &Value, label: &str, report: &mut LiveAuditReport) {
    let provider = value.get("provider").and_then(Value::as_str).unwrap_or("");
    if provider.trim().is_empty() {
        report.failures.push(format!("{label} missing provider"));
    }
    if matches!(provider, "fake" | "budget") {
        report
            .failures
            .push(format!("{label} provider {provider} is not live"));
    }
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    if model.trim().is_empty() || model == "fake-model" {
        report.failures.push(format!("{label} missing live model"));
    }
}

pub(crate) fn check_live_credential_fields(
    value: &Value,
    label: &str,
    report: &mut LiveAuditReport,
) {
    if value.get("credential_policy").and_then(Value::as_str) != Some("users-only") {
        report
            .failures
            .push(format!("{label} missing credential_policy=users-only"));
    }
    if value
        .get("credential_user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        report
            .failures
            .push(format!("{label} missing credential_user_id"));
    }
}
