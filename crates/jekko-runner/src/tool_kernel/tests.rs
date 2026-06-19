#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serde_json::Value;
    use zyal_core::{transfer_taint, ActionClass, Capability, TaintLabel, TaintSet};

    fn req<'a>(
        caps: &'a [Capability],
        input: &'a Value,
        taint: &'a TaintSet,
        action: ActionClass,
    ) -> AuthorizeRequest<'a> {
        AuthorizeRequest {
            tool_id: "code.exec",
            node_id: "tool/solve",
            capabilities: caps,
            side_effecting: false,
            command: None,
            url: None,
            input,
            incoming_taint: taint,
            action_class: action,
            sandbox_profile: "sealed",
            granted_offset: 7,
        }
    }

    #[test]
    fn a_clean_call_is_granted_a_redacted_lease() {
        let caps = [Capability::CodeExec];
        let input = json!({"src": "print(1)"});
        let clean = TaintSet::clean();
        let lease =
            authorize_tool_call(&req(&caps, &input, &clean, ActionClass::ExecShell)).unwrap();
        assert!(lease.redacted);
        assert_eq!(lease.granted_offset, 7);
        assert_eq!(lease.capabilities, vec![Capability::CodeExec]);
    }

    #[test]
    fn deny_always_capabilities_are_refused() {
        let caps = [Capability::CredentialExport];
        let input = json!({"x": 1});
        let clean = TaintSet::clean();
        let denied =
            authorize_tool_call(&req(&caps, &input, &clean, ActionClass::ExecShell)).unwrap_err();
        assert_eq!(denied.reason, DenyReason::CapabilityDenied);
    }

    #[test]
    fn network_fetch_is_refused_for_a_sealed_sandbox() {
        // M9-cont: a `sealed` sandbox provides no egress, so network.fetch is not
        // grantable — this makes the documented no-network guarantee REAL.
        let caps = [Capability::NetworkFetch];
        let input = json!({"src": "x"});
        let clean = TaintSet::clean();
        // req() builds a `sealed`-profile request.
        let denied = authorize_tool_call(&req(&caps, &input, &clean, ActionClass::RemoteModelCall))
            .unwrap_err();
        assert_eq!(denied.reason, DenyReason::CapabilityDenied);
    }

    #[test]
    fn input_carrying_a_credential_is_refused() {
        let caps = [Capability::CodeExec];
        let input = json!({"env": "OPENAI_API_KEY=sk-not-a-real-key"});
        let clean = TaintSet::clean();
        let denied =
            authorize_tool_call(&req(&caps, &input, &clean, ActionClass::ExecShell)).unwrap_err();
        assert_eq!(denied.reason, DenyReason::CredentialPolicy);
    }

    #[test]
    fn catastrophic_commands_hit_the_floor() {
        let caps = [Capability::ShellExec];
        let input = json!({"cmd": "x"});
        let clean = TaintSet::clean();
        let mut r = req(&caps, &input, &clean, ActionClass::ExecShell);
        r.command = Some("sudo rm -rf /");
        let denied = authorize_tool_call(&r).unwrap_err();
        assert_eq!(denied.reason, DenyReason::CommandFloor);
    }

    #[test]
    fn local_and_file_urls_are_refused() {
        let caps = [Capability::NetworkFetch];
        let input = json!({"u": 1});
        let clean = TaintSet::clean();
        for url in [
            "file:///etc/passwd",
            "http://localhost:8080",
            "http://127.0.0.1/x",
        ] {
            let mut r = req(&caps, &input, &clean, ActionClass::ArmHostAction);
            r.sandbox_profile = "egress"; // a network tool is not `sealed`
            r.url = Some(url);
            assert_eq!(
                authorize_tool_call(&r).unwrap_err().reason,
                DenyReason::UrlPolicy,
                "expected {url} to be denied"
            );
        }
    }

    #[test]
    fn url_with_embedded_basic_auth_is_redacted_in_the_detail() {
        let caps = [Capability::NetworkFetch];
        let input = json!({"u": 1});
        let clean = TaintSet::clean();
        let mut r = req(&caps, &input, &clean, ActionClass::ArmHostAction);
        r.sandbox_profile = "egress"; // a network tool is not `sealed`
        r.url = Some("http://user:hunter2@localhost:8080/x");
        let denied = authorize_tool_call(&r).unwrap_err();
        assert_eq!(denied.reason, DenyReason::UrlPolicy);
        assert!(
            !denied.detail.contains("hunter2"),
            "password leaked: {}",
            denied.detail
        );
        assert!(denied.detail.contains("<redacted>"));
    }

    #[test]
    fn url_with_a_credential_marker_is_refused_without_leaking_it() {
        let caps = [Capability::NetworkFetch];
        let input = json!({"u": 1});
        let clean = TaintSet::clean();
        let mut r = req(&caps, &input, &clean, ActionClass::ArmHostAction);
        r.sandbox_profile = "egress"; // a network tool is not `sealed`
        r.url = Some("https://x:OPENAI_API_KEY=sk-secret@api.example.com");
        let denied = authorize_tool_call(&r).unwrap_err();
        assert_eq!(denied.reason, DenyReason::CredentialPolicy);
        assert!(
            !denied.detail.contains("sk-secret"),
            "secret leaked: {}",
            denied.detail
        );
    }

    #[test]
    fn side_effecting_without_a_sandbox_is_refused() {
        let caps = [Capability::CodeExec];
        let input = json!({"x": 1});
        let clean = TaintSet::clean();
        let mut r = req(&caps, &input, &clean, ActionClass::ExecShell);
        r.side_effecting = true;
        r.sandbox_profile = "";
        assert_eq!(
            authorize_tool_call(&r).unwrap_err().reason,
            DenyReason::SandboxDenied
        );
    }

    #[test]
    fn tainted_data_cannot_arm_an_unsafe_action() {
        let caps = [Capability::ModelCall];
        let input = json!({"prompt": "hi"});
        let secret = TaintSet::from_labels([TaintLabel::Secret]);
        // secret → remote model is the canonical taint violation
        let denied =
            authorize_tool_call(&req(&caps, &input, &secret, ActionClass::RemoteModelCall))
                .unwrap_err();
        assert_eq!(denied.reason, DenyReason::TaintViolation);
    }

    #[test]
    fn decide_arm_is_the_host_gate() {
        let web = ActionRequest {
            node_id: "publish/0".into(),
            action_class: ActionClass::ArmHostAction,
            taint: TaintSet::from_labels([TaintLabel::WebContent]),
            description: "write a file".into(),
        };
        assert!(matches!(decide_arm(&web), ArmDecision::Blocked { .. }));
        // once sanitized, the same action arms
        let sanitized = ActionRequest {
            taint: transfer_taint(&web.taint, Some("html_strip")),
            ..web
        };
        assert_eq!(decide_arm(&sanitized), ArmDecision::Armed);
    }

    #[test]
    fn receipts_refuse_to_persist_a_secret() {
        let clean = TaintSet::clean();
        let base = ReceiptInput {
            receipt_id: "r1",
            tool_id: "code.exec",
            node_id: "tool/solve",
            phase: "succeeded",
            latency_ms: 12,
            cost_usd: 0.0,
            input: "print(1)",
            output: "1",
            taint_in: &clean,
            taint_out: &clean,
            lease_id: Some("lease-1".into()),
            deny_reason: None,
        };
        let ok = ToolReceipt::finalize(base).unwrap();
        assert_eq!(ok.input_hash.len(), 64);
        assert!(ok.taint_in.contains(&"trusted".to_string()));

        // a planted credential in the output is rejected
        let planted = ReceiptInput {
            output: "ANTHROPIC_API_KEY=sk-leak",
            input: "print(1)",
            receipt_id: "r2",
            tool_id: "code.exec",
            node_id: "tool/solve",
            phase: "succeeded",
            latency_ms: 1,
            cost_usd: 0.0,
            taint_in: &clean,
            taint_out: &clean,
            lease_id: None,
            deny_reason: None,
        };
        assert!(ToolReceipt::finalize(planted).is_err());
    }

    #[test]
    fn combine_taint_unions_inputs() {
        let a = TaintSet::from_labels([TaintLabel::WebContent]);
        let b = TaintSet::from_labels([TaintLabel::ProdData]);
        let combined = combine_taint(&[a, b]);
        assert!(combined.has(TaintLabel::WebContent) && combined.has(TaintLabel::ProdData));
    }
}
