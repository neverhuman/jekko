use serde_json::Value;

use super::{LiveAuditReport, REQUIRED_ARTIFACTS};

pub(crate) fn audit_packet(packet: Option<&Value>, report: &mut LiveAuditReport) {
    let Some(packet) = packet else {
        return;
    };
    if packet
        .get("credential_policy")
        .and_then(Value::as_str)
        .unwrap_or("")
        != "users-only"
    {
        report
            .failures
            .push("packet credential_policy must be users-only".to_string());
    }
    if !packet
        .pointer("/privacy_contract/users_only_credentials")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        report
            .failures
            .push("packet privacy_contract.users_only_credentials must be true".to_string());
    }
    for required in REQUIRED_ARTIFACTS {
        let declared = packet
            .pointer("/artifact_contract/required_artifacts")
            .and_then(Value::as_array)
            .map(|items| items.iter().any(|item| item.as_str() == Some(required)))
            .unwrap_or(false);
        if !declared {
            report.failures.push(format!(
                "packet artifact_contract.required_artifacts missing {required}"
            ));
        }
    }
}
