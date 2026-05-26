//! Round-trip durable daemon forever and port workflow rows.

use jekko_store::daemon::{
    self, DaemonConceptLinkRow, DaemonConceptRow, DaemonFindingBatchRow, DaemonFindingEdgeRow,
    DaemonFindingRow, DaemonRegressionCycleRow, DaemonRunRow, MemoryCapsuleRow, ModelOutcomeRow,
    ModelReliabilityRow, ParityCaseRow, ParityResultRow, ParityRunRow, PerfBudgetRow, PortPhaseRow,
    PortTargetRow, PortTaskRow, ReasoningArtifactRow, ReasoningEdgeRow, ReasoningLaneRow,
    RepoGraphEdgeRow, RepoGraphNodeRow,
};
use jekko_store::db::Db;
use jekko_store::project::{self, ProjectRow};
use jekko_store::session::{self, SessionRow};
use serde_json::json;

fn seed_run(db: &Db) {
    let conn = db.connection();
    project::upsert(
        conn,
        &ProjectRow {
            id: "project-1".into(),
            worktree: "/tmp/project-1".into(),
            vcs: Some("git".into()),
            name: Some("project".into()),
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: 1,
            time_updated: 1,
            time_initialized: Some(1),
            sandboxes: Vec::new(),
            commands: None,
        },
    )
    .unwrap();
    session::upsert(
        conn,
        &SessionRow {
            id: "session-1".into(),
            project_id: "project-1".into(),
            workspace_id: None,
            parent_id: None,
            slug: "session-1".into(),
            directory: "/tmp/project-1".into(),
            path: Some("/tmp/project-1/.jekko/session-1".into()),
            title: "seed".into(),
            version: "v1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            agent: None,
            model: None,
            time_created: 1,
            time_updated: 1,
            time_compacting: None,
            time_archived: None,
        },
    )
    .unwrap();
    // Existing daemon_run was created before the later session FK rebuild
    // migration. On fresh in-memory journals SQLite preserves that historical
    // FK target name, so daemon runtime tests disable FK enforcement while
    // seeding daemon rows and focus on typed row round-trips.
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    daemon::upsert_run(
        conn,
        &DaemonRunRow {
            id: "run-1".into(),
            root_session_id: "session-1".into(),
            active_session_id: "session-1".into(),
            status: "running".into(),
            phase: "drafting".into(),
            spec_json: json!({"kind": "port"}),
            spec_hash: "hash".into(),
            iteration: 1,
            epoch: 0,
            last_error: None,
            last_exit_result_json: None,
            stopped_at: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
}

#[test]
fn forever_tables_round_trip() {
    let db = Db::open_in_memory().unwrap();
    seed_run(&db);
    let conn = db.connection();

    daemon::upsert_finding(
        conn,
        &DaemonFindingRow {
            id: "finding-1".into(),
            run_id: "run-1".into(),
            iteration: 1,
            rule_id: "HLT-001".into(),
            fingerprint: "fp".into(),
            severity: "high".into(),
            paths: vec!["src/lib.rs".into()],
            cap: None,
            status: "queued".into(),
            attempt_count: 0,
            batch_id: Some("batch-1".into()),
            last_error: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_finding_batch(
        conn,
        &DaemonFindingBatchRow {
            id: "batch-1".into(),
            run_id: "run-1".into(),
            wave_index: 0,
            lane: "parallel".into(),
            worker_id: Some("worker-1".into()),
            status: "running".into(),
            started_at: Some(2),
            ended_at: None,
            result_json: Some(json!({"ok": true})),
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_finding_edge(
        conn,
        &DaemonFindingEdgeRow {
            run_id: "run-1".into(),
            parent_id: "finding-1".into(),
            child_id: "finding-2".into(),
            kind: "path_overlap".into(),
            time_created: 2,
        },
    )
    .unwrap();
    daemon::upsert_concept(
        conn,
        &DaemonConceptRow {
            id: "concept-row-1".into(),
            run_id: "run-1".into(),
            concept_id: "protocol".into(),
            definition: "wire protocol parity".into(),
            derived_from_json: Some(json!(["docs"])),
            proof_refs_json: None,
            confidence: 0.8,
            invalidated_at: None,
            invalidated_reason: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_concept_link(
        conn,
        &DaemonConceptLinkRow {
            run_id: "run-1".into(),
            parent_concept: "protocol".into(),
            child_concept: "resp".into(),
            relation: "contains".into(),
            time_created: 1,
        },
    )
    .unwrap();
    daemon::upsert_regression_cycle(
        conn,
        &DaemonRegressionCycleRow {
            id: "cycle-1".into(),
            run_id: "run-1".into(),
            iteration: 1,
            baseline_score: Some(90.0),
            current_score: Some(91.0),
            hard_delta: 0,
            soft_delta: -1,
            caps_delta: 0,
            status: "pass".into(),
            result_json: Some(json!({"score_delta": 1.0})),
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();

    assert_eq!(
        daemon::get_finding(conn, "finding-1")
            .unwrap()
            .unwrap()
            .paths,
        vec!["src/lib.rs"]
    );
    assert_eq!(
        daemon::list_finding_batches_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        daemon::list_finding_edges_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        daemon::get_concept(conn, "run-1", "protocol")
            .unwrap()
            .unwrap()
            .confidence,
        0.8
    );
    assert_eq!(
        daemon::list_concept_links_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        daemon::list_regression_cycles_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn port_workflow_tables_round_trip() {
    let db = Db::open_in_memory().unwrap();
    seed_run(&db);
    let conn = db.connection();

    daemon::upsert_port_target(
        conn,
        &PortTargetRow {
            id: "target-1".into(),
            run_id: "run-1".into(),
            target: "MiniKV".into(),
            replacement: "MiniKV Rust".into(),
            target_repo: Some("/tmp/minikv-ref".into()),
            replacement_repo: Some("/tmp/minikv-rs".into()),
            request: "port MiniKV".into(),
            status: "planned".into(),
            current_phase_id: Some("phase-1".into()),
            worker_cap: 10,
            last_audit_score: Some(95.0),
            last_parity_report_json: Some(json!({"passed": 1})),
            last_perf_gap_json: None,
            rollback_status: "clean".into(),
            quarantine_status: "none".into(),
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_port_phase(
        conn,
        &PortPhaseRow {
            id: "phase-1".into(),
            run_id: "run-1".into(),
            target_id: "target-1".into(),
            ordinal: 1,
            name: "protocol".into(),
            status: "building".into(),
            strategy: "target-switched".into(),
            plan_json: Some(json!({"tasks": ["ping"]})),
            task_count: 1,
            last_audit_score: None,
            last_parity_report_json: None,
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_port_task(
        conn,
        &PortTaskRow {
            id: "task-1".into(),
            run_id: "run-1".into(),
            phase_id: "phase-1".into(),
            title: "implement ping".into(),
            status: "assigned".into(),
            worker_id: Some("worker-1".into()),
            branch: Some("zyal/run-1/worker-1/task-1".into()),
            write_scope: vec!["src/protocol.rs".into()],
            proof_lane: Some("just zyal-port-fast".into()),
            attempt_count: 1,
            rollback_status: "clean".into(),
            quarantine_reason: None,
            last_error: None,
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_parity_case(
        conn,
        &ParityCaseRow {
            id: "resp.ping.basic".into(),
            run_id: "run-1".into(),
            target_id: "target-1".into(),
            tags: vec!["required".into(), "approved".into()],
            target_kind: "minikv".into(),
            steps_json: json!([{"send": "PING", "expect": "PONG"}]),
            perf_json: Some(json!({"p95_ms_max_ratio": 1.25})),
            approved: true,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_parity_run(
        conn,
        &ParityRunRow {
            id: "parity-run-1".into(),
            run_id: "run-1".into(),
            target_id: "target-1".into(),
            case_count: 1,
            status: "pass".into(),
            report_path: Some("target/zyal/parity/run-1/report.json".into()),
            started_at: Some(1),
            ended_at: Some(2),
            summary_json: Some(json!({"passed": 1})),
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::insert_parity_result(
        conn,
        &ParityResultRow {
            id: "result-1".into(),
            parity_run_id: "parity-run-1".into(),
            case_id: "resp.ping.basic".into(),
            target_name: "candidate".into(),
            status: "passed".into(),
            skipped: false,
            duration_ms: Some(3),
            perf_json: Some(json!({"p95_ms": 1.0})),
            message: None,
            time_created: 2,
        },
    )
    .unwrap();
    daemon::upsert_perf_budget(
        conn,
        &PerfBudgetRow {
            id: "budget-1".into(),
            run_id: "run-1".into(),
            case_id: "resp.ping.basic".into(),
            metric: "p95_ms".into(),
            max_ratio: Some(1.25),
            baseline_value: Some(1.0),
            candidate_value: Some(1.1),
            status: "pass".into(),
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_repo_graph_node(
        conn,
        &RepoGraphNodeRow {
            id: "node-file".into(),
            run_id: "run-1".into(),
            kind: "file".into(),
            key: "src/lib.rs".into(),
            label: "src/lib.rs".into(),
            payload_json: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_repo_graph_node(
        conn,
        &RepoGraphNodeRow {
            id: "node-test".into(),
            run_id: "run-1".into(),
            kind: "test".into(),
            key: "tests/ping.rs".into(),
            label: "tests/ping.rs".into(),
            payload_json: None,
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_repo_graph_edge(
        conn,
        &RepoGraphEdgeRow {
            run_id: "run-1".into(),
            src_node_id: "node-test".into(),
            dst_node_id: "node-file".into(),
            kind: "tests".into(),
            payload_json: None,
            time_created: 1,
        },
    )
    .unwrap();
    daemon::upsert_model_outcome(
        conn,
        &ModelOutcomeRow {
            id: "model-1".into(),
            run_id: "run-1".into(),
            task_id: Some("task-1".into()),
            model_id: "cheap-builder".into(),
            role: "implement".into(),
            cost_usd: Some(0.01),
            latency_ms: Some(100),
            status: "success".into(),
            reviewer_score: Some(0.9),
            winner: true,
            payload_json: Some(json!({"notes": "ok"})),
            time_created: 1,
            time_updated: 1,
        },
    )
    .unwrap();
    daemon::upsert_reasoning_artifact(
        conn,
        &ReasoningArtifactRow {
            id: "artifact-1".into(),
            run_id: "run-1".into(),
            role: "framer".into(),
            kind: "task_contract".into(),
            title: "Task contract".into(),
            summary: "Port MiniKV to Rust".into(),
            evidence_level: "external_grounding".into(),
            confidence: 0.35,
            payload_json: Some(json!({"objective": "parity"})),
            content_hash: "hash-artifact-1".into(),
            status: "verified".into(),
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_reasoning_artifact(
        conn,
        &ReasoningArtifactRow {
            id: "artifact-2".into(),
            run_id: "run-1".into(),
            role: "reducer".into(),
            kind: "master_plan".into(),
            title: "Master plan".into(),
            summary: "Generic staged plan".into(),
            evidence_level: "executable".into(),
            confidence: 0.8,
            payload_json: Some(json!({"stages": ["discover"]})),
            content_hash: "hash-artifact-2".into(),
            status: "verified".into(),
            time_created: 2,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_reasoning_edge(
        conn,
        &ReasoningEdgeRow {
            run_id: "run-1".into(),
            src_artifact_id: "artifact-1".into(),
            dst_artifact_id: "artifact-2".into(),
            kind: "supports".into(),
            weight: Some(1.0),
            payload_json: Some(json!({"basis": "reduced"})),
            time_created: 2,
        },
    )
    .unwrap();
    daemon::upsert_reasoning_lane(
        conn,
        &ReasoningLaneRow {
            id: "lane-1".into(),
            run_id: "run-1".into(),
            role: "planner".into(),
            strategy: "minimal".into(),
            status: "complete".into(),
            artifact_ids: vec!["artifact-1".into()],
            write_scope: vec!["src/**".into()],
            worker_id: Some("worker-1".into()),
            confidence: 0.35,
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::upsert_memory_capsule(
        conn,
        &MemoryCapsuleRow {
            id: "memory-1".into(),
            run_id: "run-1".into(),
            artifact_id: "artifact-2".into(),
            scope: "repo".into(),
            status: "verified".into(),
            summary: "Generic staged port plans are target-derived at runtime".into(),
            evidence_level: "executable".into(),
            confidence: 0.8,
            payload_json: Some(json!({"do_not": "hard-code target stages"})),
            content_hash: "hash-memory-1".into(),
            time_created: 2,
            time_updated: 2,
            memory_kind: "semantic".into(),
            promotion_status: "scratch".into(),
            claim_text: String::new(),
            approved_by_role: None,
            embedding: None,
        },
    )
    .unwrap();
    daemon::upsert_model_reliability(
        conn,
        &ModelReliabilityRow {
            model_id: "cheap-builder".into(),
            role: "builder".into(),
            task_kind: "implement".into(),
            success_count: 2,
            failure_count: 1,
            winner_count: 1,
            total_latency_ms: 300,
            total_cost_usd: 0.03,
            score: 0.71,
            time_created: 1,
            time_updated: 2,
        },
    )
    .unwrap();
    daemon::record_model_reliability_outcome(
        conn,
        "cheap-builder",
        "builder",
        "implement",
        true,
        true,
        50,
        0.01,
        3,
    )
    .unwrap();

    assert_eq!(
        daemon::list_port_targets_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(daemon::list_runs(conn, 10).unwrap()[0].id, "run-1");
    assert_eq!(
        daemon::get_port_target(conn, "target-1")
            .unwrap()
            .unwrap()
            .worker_cap,
        10
    );
    assert_eq!(
        daemon::list_port_phases_for_target(conn, "target-1").unwrap()[0].status,
        "building"
    );
    assert_eq!(
        daemon::get_port_task(conn, "task-1")
            .unwrap()
            .unwrap()
            .write_scope,
        vec!["src/protocol.rs"]
    );
    assert!(daemon::list_parity_cases_for_target(conn, "target-1").unwrap()[0].approved);
    assert_eq!(
        daemon::list_parity_results_for_run(conn, "parity-run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        daemon::list_parity_runs_for_target(conn, "target-1").unwrap()[0].id,
        "parity-run-1"
    );
    assert_eq!(
        daemon::list_repo_graph_nodes_for_run(conn, "run-1")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        daemon::list_repo_graph_edges_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert!(daemon::list_model_outcomes_for_run(conn, "run-1").unwrap()[0].winner);
    assert_eq!(
        daemon::list_reasoning_artifacts_for_run(conn, "run-1")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        daemon::list_reasoning_edges_for_run(conn, "run-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        daemon::list_reasoning_lanes_for_run(conn, "run-1").unwrap()[0].artifact_ids,
        vec!["artifact-1"]
    );
    assert_eq!(
        daemon::list_memory_capsules_for_run(conn, "run-1").unwrap()[0].status,
        "verified"
    );
    assert!(
        daemon::get_model_reliability(conn, "cheap-builder", "builder", "implement")
            .unwrap()
            .unwrap()
            .score
            > 0.7
    );
    assert_eq!(daemon::list_model_reliability(conn, None).unwrap().len(), 1);
}
