#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventKind;
    use crate::run_store::RunContext;
    use crate::run_store::SourceMode;
    use crate::source_runtime::FeedKind;
    use crate::tool_kernel::ToolRegistry;
    use crate::tournament::{candidate_hash, BlindBallot};
    use paper_builder::{PaperBuildMode, PaperBuildRequest, GLOBAL_REF};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn descriptor(tools_block: Value, id: &str) -> ToolDescriptor {
        ToolRegistry::from_block(&tools_block)
            .lookup(id)
            .cloned()
            .expect("descriptor")
    }

    fn feed_spec(id: &str) -> DataFeedSpec {
        DataFeedSpec {
            id: id.to_string(),
            kind: FeedKind::StockTicker,
            source: "fixture://prices".to_string(),
            symbols: vec!["ACME".to_string()],
            value_schema_id: "price.v1".to_string(),
            primary_field: "value".to_string(),
            poll_interval_ms: 1000,
            taint: TaintSet::from_labels([zyal_core::TaintLabel::WebContent]),
        }
    }

    fn cand(id: &str, provider: &str, family: &str, content: &str) -> Candidate {
        Candidate {
            candidate_id: id.to_string(),
            provider: provider.to_string(),
            family: family.to_string(),
            content: content.to_string(),
        }
    }

    fn ballot(content: &str, rank: u32) -> BlindBallot {
        BlindBallot {
            candidate_hash: candidate_hash(content),
            rank,
        }
    }

    fn event_kinds(repo: &Path, run_id: &str) -> Vec<String> {
        read_events(repo, run_id)
            .into_iter()
            .filter_map(|v| v.get("kind").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    fn read_events(repo: &Path, run_id: &str) -> Vec<Value> {
        let path = repo.join(format!("target/zyal/runs/{run_id}/events.jsonl"));
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect()
    }

    fn phases_for(repo: &Path, run_id: &str, node_id: &str) -> Vec<String> {
        read_events(repo, run_id)
            .into_iter()
            .filter(|v| v.get("kind").and_then(Value::as_str) == Some("tool_call_update"))
            .filter(|v| v.pointer("/data/node_id").and_then(Value::as_str) == Some(node_id))
            .filter_map(|v| {
                v.pointer("/data/phase")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn dispatches_all_four_kernels_in_fake_mode_and_emits_events() {
        let dir = tempfile::tempdir().unwrap();
        let tools = json!({ "code.exec": { "kind": "code", "node_type": "tool", "side_effecting": false } });
        let nodes = vec![
            DispatchNode::Tool {
                descriptor: descriptor(tools.clone(), "code.exec"),
                node_id: "tool/solve".to_string(),
                input: json!({ "src": "print(2 + 2)" }),
                incoming_taint: TaintSet::clean(),
                action_class: ActionClass::ExecShell,
            },
            DispatchNode::Feed {
                spec: feed_spec("px"),
                seq: 0,
            },
            DispatchNode::Route {
                route_id: "r1".to_string(),
                policy: "cascade".to_string(),
                candidates: vec![
                    RouteCandidate {
                        candidate_id: "primary".to_string(),
                        cost_usd: 0.20,
                    },
                    RouteCandidate {
                        candidate_id: "cheap".to_string(),
                        cost_usd: 0.05,
                    },
                ],
            },
            DispatchNode::Tournament {
                tournament_id: "t1".to_string(),
                candidates: vec![
                    cand("c0", "openai", "gpt", "answer A"),
                    cand("c1", "anthropic", "claude", "answer B"),
                ],
                judges: vec![
                    JudgeInput {
                        provider: "mistral".into(),
                        family: "mixtral".into(),
                        ranking: vec![ballot("answer B", 1), ballot("answer A", 2)],
                    },
                    JudgeInput {
                        provider: "meta".into(),
                        family: "llama".into(),
                        ranking: vec![ballot("answer B", 1), ballot("answer A", 2)],
                    },
                ],
            },
        ];

        let report = run_flow_dispatch(dir.path(), "run-1", SourceMode::Fake, &nodes).unwrap();
        let kinds = event_kinds(dir.path(), "run-1");
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);

        // Tool: full happy-path phase sequence emitted.
        assert_eq!(
            phases_for(dir.path(), "run-1", "tool/solve"),
            vec!["queued", "authorizing", "started", "succeeded"]
        );
        assert!(report.tools[0].output.is_some());

        // Feed: one tick frame.
        assert!(kinds.iter().any(|k| k == "feed_tick"));
        assert_eq!(report.feeds.len(), 1);

        // Route: decision → 2 provider calls → winner = the cheapest candidate.
        assert_eq!(kinds.iter().filter(|k| *k == "provider_call").count(), 2);
        assert!(kinds.iter().any(|k| k == "routing_decision"));
        assert_eq!(report.routes[0].1.as_deref(), Some("cheap"));

        // Tournament: a winner is judged, but promotion is FAIL-CLOSED without a
        // real verifier — judging happened, promotion did not.
        assert!(kinds.iter().any(|k| k == "tournament_generation"));
        assert!(kinds.iter().any(|k| k == "promotion_gate"));
        assert_eq!(report.tournaments[0].winner_id.as_deref(), Some("c1"));
        assert!(!report.tournaments[0].promoted);
        assert!(!report.tournaments[0].verified);
    }

    #[test]
    fn replay_mode_does_not_invoke_the_tool_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let tools = json!({ "code.exec": { "kind": "code", "side_effecting": false } });
        let nodes = vec![DispatchNode::Tool {
            descriptor: descriptor(tools, "code.exec"),
            node_id: "tool/solve".to_string(),
            input: json!({ "src": "print(1)" }),
            incoming_taint: TaintSet::clean(),
            action_class: ActionClass::ExecShell,
        }];
        let report =
            run_flow_dispatch(dir.path(), "run-replay", SourceMode::Replay, &nodes).unwrap();
        let phases = phases_for(dir.path(), "run-replay", "tool/solve");
        assert!(phases.contains(&"cached".to_string()));
        assert!(!phases.contains(&"succeeded".to_string()));
        assert!(report.tools[0].output.is_none());
    }

    #[test]
    fn a_sealed_sandbox_network_tool_is_denied_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let tools = json!({
            "fetcher": { "kind": "shell", "sandbox": "sealed", "side_effecting": true, "capabilities": ["network.fetch"] }
        });
        let nodes = vec![DispatchNode::Tool {
            descriptor: descriptor(tools, "fetcher"),
            node_id: "tool/fetch".to_string(),
            input: json!({ "cmd": "curl example.com" }),
            incoming_taint: TaintSet::clean(),
            action_class: ActionClass::ArmHostAction,
        }];
        let report = run_flow_dispatch(dir.path(), "run-deny", SourceMode::Fake, &nodes).unwrap();
        let phases = phases_for(dir.path(), "run-deny", "tool/fetch");
        assert!(
            phases.contains(&"denied".to_string()),
            "phases were {phases:?}"
        );
        assert!(report.tools[0].denied.is_some());
        assert!(report.tools[0].output.is_none());
    }

    #[test]
    fn an_over_budget_frame_is_isolated_and_does_not_abort_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let tools = json!({ "code.exec": { "kind": "code", "side_effecting": false } });
        // A pathologically long node id pushes the ToolCallUpdate frame past the
        // 512-byte EventSink budget; the emit fails but is recorded, not fatal.
        let huge = "n".repeat(600);
        let nodes = vec![
            DispatchNode::Tool {
                descriptor: descriptor(tools.clone(), "code.exec"),
                node_id: huge,
                input: json!({ "src": "print(1)" }),
                incoming_taint: TaintSet::clean(),
                action_class: ActionClass::ExecShell,
            },
            // A normal node AFTER the bad one must still dispatch + emit.
            DispatchNode::Feed {
                spec: feed_spec("px"),
                seq: 0,
            },
        ];
        let report = run_flow_dispatch(dir.path(), "run-huge", SourceMode::Fake, &nodes).unwrap();
        assert!(
            !report.errors.is_empty(),
            "the over-budget frame should be recorded"
        );
        // The kernel outcome is still captured despite the emission failure.
        assert_eq!(report.tools.len(), 1);
        // The following node ran and emitted normally.
        assert_eq!(report.feeds.len(), 1);
        assert!(event_kinds(dir.path(), "run-huge")
            .iter()
            .any(|k| k == "feed_tick"));
    }

    /// A compiled FlowGraph IR shaped like zyalc's emitter output: a router that
    /// fanned out to two provider_call boxes, a data_feed, and non-executable
    /// topology (an agent + a watcher) that must be skipped.
    fn flowgraph_ir() -> Value {
        json!({
            "ir_version": "flowgraph.v3",
            "nodes": [
                { "id": "planner", "node_type": "agent", "label": "Planner" },
                { "id": "px", "node_type": "data_feed", "label": "Market feed",
                  "data_feed": { "kind": "stock_ticker", "symbols": ["ACME"], "primary": "value" } },
                { "id": "route_fusion", "node_type": "router", "label": "fusion route",
                  "router": { "drafts": 2, "fusion": 1, "strategy": "fusion_sample" } },
                { "id": "route_fusion/call-0", "node_type": "provider_call", "label": "provider 0" },
                { "id": "route_fusion/call-1", "node_type": "provider_call", "label": "provider 1" },
                { "id": "watch", "node_type": "watcher", "label": "Convergence" }
            ]
        })
    }

    #[test]
    fn node_to_dispatch_maps_only_executable_nodes() {
        let ir = flowgraph_ir();
        let nodes = ir.get("nodes").and_then(Value::as_array).unwrap();
        let kinds: Vec<&str> = nodes
            .iter()
            .map(|n| n.get("node_type").unwrap().as_str().unwrap())
            .collect();
        let mapped: Vec<Option<&'static str>> = nodes
            .iter()
            .map(|n| match node_to_dispatch(n, nodes) {
                Some(DispatchNode::Feed { .. }) => Some("feed"),
                Some(DispatchNode::Route { .. }) => Some("route"),
                Some(DispatchNode::Tool { .. }) => Some("tool"),
                Some(DispatchNode::Tournament { .. }) => Some("tournament"),
                Some(DispatchNode::PaperBuilder { .. }) => Some("paper_builder"),
                None => None,
            })
            .collect();
        // agent + provider_call children + watcher are NOT directly executable.
        assert_eq!(
            kinds.iter().zip(&mapped).collect::<Vec<_>>(),
            vec![
                (&"agent", &None),
                (&"data_feed", &Some("feed")),
                (&"router", &Some("route")),
                (&"provider_call", &None),
                (&"provider_call", &None),
                (&"watcher", &None),
            ]
        );
    }

    #[test]
    fn dispatch_flowgraph_runs_the_executable_nodes_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let report =
            dispatch_flowgraph(&flowgraph_ir(), dir.path(), "run-fg", SourceMode::Fake).unwrap();
        assert!(report.errors.is_empty());
        // The feed ticked and the router emitted its decision → 2 provider calls → winner.
        assert_eq!(report.feeds.len(), 1);
        assert_eq!(report.routes.len(), 1);
        assert_eq!(report.routes[0].0, "route_fusion");
        assert_eq!(report.routes[0].1.as_deref(), Some("route_fusion/call-0")); // earliest on uniform cost
        let kinds = event_kinds(dir.path(), "run-fg");
        assert!(kinds.iter().any(|k| k == "feed_tick"));
        assert!(kinds.iter().any(|k| k == "routing_decision"));
        assert_eq!(kinds.iter().filter(|k| *k == "provider_call").count(), 2);
        assert!(kinds.iter().any(|k| k == "route_winner"));
        // no tools/tournaments dispatched from the static IR
        assert!(report.tools.is_empty() && report.tournaments.is_empty());
    }

    #[test]
    fn dispatch_flowgraph_is_a_noop_for_a_graph_with_no_executable_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let ir = json!({ "nodes": [ { "id": "a", "node_type": "agent" }, { "id": "s", "node_type": "supervisor" } ] });
        let report = dispatch_flowgraph(&ir, dir.path(), "run-empty", SourceMode::Fake).unwrap();
        assert!(report.feeds.is_empty() && report.routes.is_empty() && report.errors.is_empty());
    }

    fn paper_builder_ir() -> Value {
        json!({
            "ir_version": "flowgraph.v3",
            "nodes": [
                {
                    "id": "paper",
                    "node_type": "paper_builder",
                    "label": "paper",
                    "workflow_call": {
                        "ref": "zyal://global/paper-builder@1",
                        "source_hash": "sha256:source",
                        "interface_hash": "sha256:iface",
                        "budget_usd": 3.0
                    },
                    "paper_builder": {
                        "mode": "medium",
                        "journal_target": "ieee",
                        "paper_goal": "prove deterministic paper artifacts",
                        "data_artifacts": ["target/results.csv"],
                        "success_criteria": ["latex_clean"],
                        "authors": [
                            {
                                "name": "Ada Lovelace",
                                "affiliation": "Analytical Engine Lab",
                                "email": "ada@example.org"
                            }
                        ],
                        "output_dir": "target/zyal/papers/${run_id}"
                    }
                }
            ]
        })
    }

    fn paper_request() -> PaperBuildRequest {
        let ir = paper_builder_ir();
        let nodes = ir.get("nodes").and_then(Value::as_array).unwrap();
        serde_json::from_value(nodes[0]["paper_builder"].clone()).unwrap()
    }

    fn frame_reason(frames: &[(EventKind, Value)], kind: EventKind) -> Option<String> {
        frames
            .iter()
            .find(|(candidate, _)| *candidate == kind)
            .and_then(|(_, data)| data.get("reason").and_then(Value::as_str))
            .map(str::to_string)
    }

    #[test]
    fn node_to_dispatch_maps_paper_builder() {
        let ir = paper_builder_ir();
        let nodes = ir.get("nodes").and_then(Value::as_array).unwrap();
        match node_to_dispatch(&nodes[0], nodes) {
            Some(DispatchNode::PaperBuilder {
                workflow_ref,
                request,
                ..
            }) => {
                assert_eq!(workflow_ref, "zyal://global/paper-builder@1");
                assert_eq!(request.mode, PaperBuildMode::Medium);
            }
            _ => panic!("paper_builder node should dispatch"),
        }
    }

    #[test]
    fn dispatch_flowgraph_runs_paper_builder_and_writes_fixture_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let report = dispatch_flowgraph(
            &paper_builder_ir(),
            dir.path(),
            "paper-run",
            SourceMode::Fake,
        )
        .unwrap();
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
        assert_eq!(report.paper_builds.len(), 1);
        let out = dir.path().join("target/zyal/papers/paper-run");
        for rel in [
            "main.tex",
            "sections/introduction.tex",
            "references.bib",
            "paper.pdf",
            "arxiv.tar.gz",
            "build_receipt.json",
            "ledgers/research.jsonl",
            "ledgers/review.jsonl",
        ] {
            assert!(out.join(rel).exists(), "missing {rel}");
        }
        let kinds = event_kinds(dir.path(), "paper-run");
        assert!(kinds.iter().any(|k| k == "child_workflow_started"));
        assert!(kinds.iter().any(|k| k == "child_workflow_completed"));
        assert_eq!(
            kinds
                .iter()
                .filter(|k| *k == "review_epoch_completed")
                .count(),
            1
        );
        assert_eq!(
            kinds.iter().filter(|k| *k == "artifact_published").count(),
            3
        );
        assert!(report.paper_builds[0].latex_verified);
        assert_eq!(
            report.paper_builds[0].arxiv_tar,
            "target/zyal/papers/paper-run/arxiv.tar.gz"
        );
    }

    #[test]
    fn replay_mode_does_not_write_paper_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let report = dispatch_flowgraph(
            &paper_builder_ir(),
            dir.path(),
            "paper-replay",
            SourceMode::Replay,
        )
        .unwrap();
        assert!(report.paper_builds.is_empty());
        assert!(
            report.errors.iter().any(|err| err.contains("cached")),
            "errors: {:?}",
            report.errors
        );
        assert!(!dir.path().join("target/zyal/papers/paper-replay").exists());

        let events = read_events(dir.path(), "paper-replay");
        assert!(events.iter().any(|event| {
            event.get("kind").and_then(Value::as_str) == Some("child_workflow_failed")
                && event.pointer("/data/reason").and_then(Value::as_str) == Some("cached_required")
        }));
        assert!(!event_kinds(dir.path(), "paper-replay")
            .iter()
            .any(|kind| kind == "child_workflow_completed"));
    }

    #[test]
    fn paper_builder_write_failure_emits_failed_event() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("target/zyal/papers/paper-write-fail");
        std::fs::create_dir_all(out.join("paper.pdf")).unwrap();

        let report = dispatch_flowgraph(
            &paper_builder_ir(),
            dir.path(),
            "paper-write-fail",
            SourceMode::Fake,
        )
        .unwrap();
        assert!(report.paper_builds.is_empty());
        assert!(
            report.errors.iter().any(|err| err.contains("paper.pdf")),
            "errors: {:?}",
            report.errors
        );
        let events = read_events(dir.path(), "paper-write-fail");
        assert!(events.iter().any(|event| {
            event.get("kind").and_then(Value::as_str) == Some("child_workflow_failed")
                && event.pointer("/data/reason").and_then(Value::as_str) == Some("write_pdf")
        }));
        assert!(!event_kinds(dir.path(), "paper-write-fail")
            .iter()
            .any(|kind| kind == "child_workflow_completed"));
    }

    #[test]
    fn paper_output_dir_rejects_unsafe_final_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_paper_output_dir(dir.path(), "run-1", "/tmp/paper").is_err());
        assert!(resolve_paper_output_dir(dir.path(), "run-1", "target/../paper").is_err());
        assert!(resolve_paper_output_dir(dir.path(), "../escape", "target/${run_id}").is_err());

        let ctx = RunContext {
            run_id: "../escape".to_string(),
            mode: SourceMode::Fake,
        };
        let (receipt, frames, err) = plan_paper_builder(
            dir.path(),
            "paper",
            &paper_request(),
            GLOBAL_REF,
            "sha256:source",
            "sha256:iface",
            &ctx,
        );
        assert!(receipt.is_none());
        assert!(err.unwrap().contains("run_id"));
        assert_eq!(
            frame_reason(&frames, EventKind::ChildWorkflowFailed).as_deref(),
            Some("unsafe_output_dir")
        );
    }

    #[cfg(unix)]
    #[test]
    fn paper_output_dir_rejects_existing_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("paper-out")).unwrap();
        assert!(resolve_paper_output_dir(dir.path(), "run-1", "paper-out/${run_id}").is_err());
    }

    #[test]
    fn paper_artifact_contract_rejects_incomplete_runner_bundle() {
        let artifacts = BTreeMap::from([
            ("main.tex".to_string(), "main.tex".to_string()),
            (
                "sections/introduction.tex".to_string(),
                "sections/introduction.tex".to_string(),
            ),
            ("references.bib".to_string(), "references.bib".to_string()),
            (
                "figures/placeholder.eps".to_string(),
                "figures/placeholder.eps".to_string(),
            ),
            (
                "tables/results.tex".to_string(),
                "tables/results.tex".to_string(),
            ),
            ("arxiv.tar.gz".to_string(), "arxiv.tar.gz".to_string()),
            (
                "build_receipt.json".to_string(),
                "build_receipt.json".to_string(),
            ),
            (
                "ledgers/research.jsonl".to_string(),
                "ledgers/research.jsonl".to_string(),
            ),
            (
                "ledgers/review.jsonl".to_string(),
                "ledgers/review.jsonl".to_string(),
            ),
        ]);
        let err = validate_paper_artifact_contract(&artifacts).unwrap_err();
        assert!(err.contains("paper.pdf"), "error: {err}");
    }

    #[test]
    fn pick_route_winner_is_lowest_cost_earliest_on_ties() {
        let cs = |v: &[(&str, f64)]| {
            v.iter()
                .map(|(id, c)| RouteCandidate {
                    candidate_id: id.to_string(),
                    cost_usd: *c,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            pick_route_winner(&cs(&[("a", 0.3), ("b", 0.1), ("c", 0.2)])),
            Some(1)
        );
        assert_eq!(pick_route_winner(&cs(&[("a", 0.1), ("b", 0.1)])), Some(0));
        assert_eq!(
            pick_route_winner(&cs(&[("a", f64::NAN), ("b", 0.5)])),
            Some(1)
        );
        assert_eq!(pick_route_winner(&[]), None);
    }
}
