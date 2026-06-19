#[cfg(test)]
mod m9_tests {
    use super::*;
    use crate::run_store::SourceMode;
    use serde_json::json;
    use serde_json::Value;
    use zyal_core::{ActionClass, Capability, TaintSet};

    fn code_descriptor() -> ToolDescriptor {
        ToolDescriptor::from_value(
            "code.exec",
            &json!({ "kind": "code", "node_type": "tool", "side_effecting": false,
                     "capabilities": ["code.exec"], "sandbox": "sealed", "deterministic": true }),
        )
    }

    #[test]
    fn registry_parses_and_looks_up_deterministically() {
        let reg = ToolRegistry::from_block(&json!({
            "code.exec": { "kind": "code", "capabilities": ["code.exec"] },
            "calculator": { "kind": "builtin" }
        }));
        assert_eq!(reg.ids(), vec!["calculator", "code.exec"]); // sorted
        assert_eq!(reg.lookup("code.exec").unwrap().kind, ToolKind::Code);
        assert!(reg.lookup("missing").is_none());
    }

    #[test]
    fn code_exec_fake_is_deterministic() {
        let a = CodeExecAdapter::new("code.exec", true);
        let lease = ToolLease {
            lease_id: "l".into(),
            tool_id: "code.exec".into(),
            node_id: "n".into(),
            capabilities: vec![Capability::CodeExec],
            sandbox_profile: "sealed".into(),
            granted_offset: 0,
            redacted: true,
        };
        let o1 = a.invoke(&lease, &json!({"src": "print(1)"})).unwrap();
        let o2 = a.invoke(&lease, &json!({"src": "print(1)"})).unwrap();
        assert_eq!(o1.output, o2.output);
        assert!(o1.output.starts_with("fake-exec:"));
    }

    fn test_lease(tool_id: &str, cap: Capability) -> ToolLease {
        ToolLease {
            lease_id: "l".into(),
            tool_id: tool_id.into(),
            node_id: "n".into(),
            capabilities: vec![cap],
            sandbox_profile: "sealed".into(),
            granted_offset: 0,
            redacted: true,
        }
    }

    #[test]
    fn sealed_sandbox_runs_and_captures_output() {
        let p = SandboxPolicy {
            interpreter: "sh".into(),
            timeout_ms: 5_000,
            max_output_bytes: 1024,
        };
        let out = run_sealed("echo hello-sandbox", &p).expect("run");
        assert!(out.output.contains("hello-sandbox"), "got {}", out.output);
    }

    #[test]
    fn sealed_sandbox_clears_inherited_env() {
        let p = SandboxPolicy {
            interpreter: "sh".into(),
            timeout_ms: 5_000,
            max_output_bytes: 1024,
        };
        let out = run_sealed("echo home=[$HOME] path=[${PATH:+yes}]", &p).expect("run");
        assert!(
            out.output.contains("home=[]"),
            "HOME must be cleared, got {}",
            out.output
        );
        assert!(
            out.output.contains("path=[yes]"),
            "PATH must be present, got {}",
            out.output
        );
    }

    #[test]
    fn sealed_sandbox_enforces_timeout() {
        let p = SandboxPolicy {
            interpreter: "sh".into(),
            timeout_ms: 150,
            max_output_bytes: 1024,
        };
        let err = run_sealed("sleep 5", &p).unwrap_err();
        assert!(err.contains("timeout"), "got {err}");
    }

    #[test]
    fn cap_output_truncates_on_a_char_boundary() {
        let capped = cap_output("abcdefghij", 4);
        assert!(
            capped.starts_with("abcd") && capped.contains("truncated"),
            "got {capped}"
        );
        assert_eq!(cap_output("short", 100), "short");
    }

    #[test]
    fn theorem_prover_proves_and_rejects() {
        let a = TheoremProverAdapter::new("theorem.prover");
        let lease = test_lease("theorem.prover", Capability::CodeExec);
        let good = json!({
            "goal": "P",
            "proof": { "steps": [
                { "justification": "axiom", "conclusion": "Q" },
                { "justification": "modus ponens", "conclusion": "P" }
            ]}
        });
        assert!(a
            .invoke(&lease, &good)
            .unwrap()
            .output
            .contains("\"proof_status\":\"proved\""));
        let bad = json!({ "goal": "P", "proof": { "steps": [{ "conclusion": "P" }] } });
        assert!(a.invoke(&lease, &bad).unwrap().output.contains("unproved"));
        let mismatch = json!({ "goal": "P", "proof": { "steps": [{ "justification": "x", "conclusion": "Q" }] } });
        assert!(a
            .invoke(&lease, &mismatch)
            .unwrap()
            .output
            .contains("unproved"));
    }

    #[test]
    fn workflow_call_respects_budget() {
        let a = WorkflowCallAdapter::new("workflow.call");
        let lease = test_lease("workflow.call", Capability::WorkflowCall);
        let ok = a
            .invoke(&lease, &json!({ "workflow": "child", "budget_usd": 1.0 }))
            .unwrap();
        assert!(ok.output.contains("child-result:") && ok.cost_usd > 0.0);
        assert!(a
            .invoke(&lease, &json!({ "workflow": "child", "budget_usd": 0.0 }))
            .is_err());
        assert!(a.invoke(&lease, &json!({ "budget_usd": 1.0 })).is_err());
    }

    #[test]
    fn adapter_for_dispatches_by_kind_and_id() {
        // code → sealed sandbox (FAKE under test).
        let code = ToolDescriptor::from_value("code.exec", &json!({ "kind": "code" }));
        let cl = test_lease("code.exec", Capability::CodeExec);
        assert!(adapter_for(&code)
            .invoke(&cl, &json!({"src": "x"}))
            .unwrap()
            .output
            .starts_with("fake-exec:"));
        // workflow → workflow.call.
        let wf = ToolDescriptor::from_value("workflow.call", &json!({ "kind": "workflow" }));
        let wl = test_lease("workflow.call", Capability::WorkflowCall);
        assert!(adapter_for(&wf)
            .invoke(&wl, &json!({"workflow": "c", "budget_usd": 1.0}))
            .unwrap()
            .output
            .contains("child-result:"));
        // theorem.prover dispatched by tool_id.
        let tp = ToolDescriptor::from_value("theorem.prover", &json!({ "kind": "builtin" }));
        let tl = test_lease("theorem.prover", Capability::CodeExec);
        assert!(adapter_for(&tp)
            .invoke(&tl, &json!({"goal": "P", "proof": {"steps": [{"justification": "a", "conclusion": "P"}]}}))
            .unwrap()
            .output
            .contains("proved"));
    }

    fn ctx<'a>(d: &'a ToolDescriptor, input: &'a Value, taint: &'a TaintSet) -> RunToolCtx<'a> {
        RunToolCtx {
            descriptor: d,
            node_id: "tool/solve",
            input,
            incoming_taint: taint,
            action_class: ActionClass::ExecShell,
            granted_offset: 1,
            receipt_id: "r1",
            source_mode: SourceMode::Live,
        }
    }

    #[test]
    fn run_tool_in_replay_mode_never_invokes_the_adapter() {
        use std::cell::Cell;
        // A spy adapter that records whether `invoke` was ever called.
        struct Spy {
            id: String,
            called: Cell<bool>,
        }
        impl ToolAdapter for Spy {
            fn tool_id(&self) -> &str {
                &self.id
            }
            fn invoke(&self, _l: &ToolLease, _i: &Value) -> Result<ToolOutput, String> {
                self.called.set(true);
                Ok(ToolOutput {
                    output: "must-not-run".into(),
                    cost_usd: 0.0,
                    latency_ms: 0,
                })
            }
        }
        let d = code_descriptor();
        let input = json!({"src": "print(1)"});
        let clean = TaintSet::clean();
        let mut c = ctx(&d, &input, &clean);
        c.source_mode = SourceMode::Replay;
        let spy = Spy {
            id: d.tool_id.clone(),
            called: Cell::new(false),
        };
        let out = run_tool(&c, &spy);
        assert!(!spy.called.get(), "replay must NOT invoke the adapter");
        assert!(out.output.is_none(), "replay yields no fresh output");
        assert!(out.phases.contains(&ToolPhase::Cached));
        assert!(out.denied.is_none() && out.lease.is_some());
    }

    #[test]
    fn run_tool_succeeds_with_phases_and_a_receipt() {
        let d = code_descriptor();
        let input = json!({"src": "print(1)"});
        let clean = TaintSet::clean();
        let adapter = CodeExecAdapter::new("code.exec", true);
        let out = run_tool(&ctx(&d, &input, &clean), &adapter);
        assert_eq!(
            out.phases,
            vec![
                ToolPhase::Queued,
                ToolPhase::Authorizing,
                ToolPhase::Started,
                ToolPhase::Succeeded
            ]
        );
        assert!(out.lease.is_some() && out.denied.is_none());
        assert_eq!(out.receipt.input_hash.len(), 64);
        assert!(out.output.unwrap().output.starts_with("fake-exec:"));
    }

    #[test]
    fn run_tool_denies_a_credential_export_capability() {
        let d = ToolDescriptor::from_value(
            "leaky",
            &json!({ "kind": "code", "capabilities": ["credential.export"] }),
        );
        let input = json!({"src": "x"});
        let clean = TaintSet::clean();
        let adapter = CodeExecAdapter::new("leaky", true);
        let out = run_tool(&ctx(&d, &input, &clean), &adapter);
        assert!(out.phases.contains(&ToolPhase::Denied));
        assert_eq!(out.denied.unwrap().reason, DenyReason::CapabilityDenied);
    }

    #[test]
    fn run_tool_flags_a_tool_that_emits_a_credential_as_misused() {
        struct Leaky;
        impl ToolAdapter for Leaky {
            fn tool_id(&self) -> &str {
                "leaky"
            }
            fn invoke(&self, _l: &ToolLease, _i: &Value) -> Result<ToolOutput, String> {
                Ok(ToolOutput {
                    output: "ANTHROPIC_API_KEY=sk-leak".into(),
                    cost_usd: 0.0,
                    latency_ms: 0,
                })
            }
        }
        let d = code_descriptor();
        let input = json!({"src": "x"});
        let clean = TaintSet::clean();
        let out = run_tool(&ctx(&d, &input, &clean), &Leaky);
        assert!(out.phases.contains(&ToolPhase::Misused));
        assert!(out.output.is_none(), "leaked output must not be returned");
        assert!(!out.receipt.output_hash.is_empty());
    }

    #[test]
    fn run_tool_failed_path_with_credential_in_error_does_not_panic_or_leak() {
        struct ErrLeak;
        impl ToolAdapter for ErrLeak {
            fn tool_id(&self) -> &str {
                "errleak"
            }
            fn invoke(&self, _l: &ToolLease, _i: &Value) -> Result<ToolOutput, String> {
                Err("sandbox failed: OPENAI_API_KEY=sk-leak in env".into())
            }
        }
        let d = code_descriptor();
        let input = json!({"src": "x"});
        let clean = TaintSet::clean();
        let out = run_tool(&ctx(&d, &input, &clean), &ErrLeak);
        assert!(out.phases.contains(&ToolPhase::Failed));
        // a receipt was produced (no panic) and the credential never persisted
        let redacted = crate::hashing::sha256_hex("<redacted: credential in output>".as_bytes());
        assert_eq!(out.receipt.output_hash, redacted);
    }

    #[test]
    fn misuse_guard_fires_on_repeats_and_error_bursts() {
        let mut g = ToolMisuseGuard::default();
        assert!(g.observe("h", false, 0.0).is_none());
        assert!(g.observe("h", false, 0.0).is_none());
        assert_eq!(
            g.observe("h", false, 0.0),
            Some("repeated_input_no_progress")
        );

        let mut e = ToolMisuseGuard::default();
        for i in 0..3 {
            assert!(e.observe(&format!("h{i}"), true, 0.0).is_none());
        }
        assert_eq!(e.observe("h4", true, 0.0), Some("error_burst"));

        // cost-runaway (M9-cont): cumulative cost crosses the ceiling.
        let mut c = ToolMisuseGuard::with_cost_ceiling(0.05);
        assert!(c.observe("a", false, 0.03).is_none());
        assert_eq!(c.observe("b", false, 0.03), Some("cost_runaway"));
    }
}
