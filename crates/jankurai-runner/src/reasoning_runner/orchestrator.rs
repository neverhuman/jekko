//! Advanced reasoning state machine orchestrator. The early phases live in
//! [`super::phases`]; this file owns setup, parity execution, optional
//! benchmark, and finalization.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::model_client::ModelClient;
use crate::model_policy::ModelTaskKind;
use crate::parity_lab::{run_target_switched_cases, write_report_artifacts, FakeTargetAdapter};
use crate::port::{PortRuntimeOptions, PortTargetRequest};
use crate::reasoning::{
    stable_reasoning_hash, AdvancedReasoningConfig, EvidenceLevel, MemoryCapsule,
    ReasoningArtifactKind, ReasoningRole,
};
use crate::reasoning_benchmark::{finish_tournament_score, score_baseline, write_benchmark_report};
use crate::reasoning_io::{
    artifact, complete_structured, emit_state, export_reasoning_graph, persist_artifact,
    persist_edge,
};
use crate::stage0_proof::{benchmark_prompt, generate_seed_cases};

use super::phases::{run_early_phases, EarlyPhasesState};
use super::types::{AdvancedReasoningSummary, AdvancedReasoningTickReport};

/// Run the advanced reasoning state machine once.
#[allow(clippy::too_many_arguments)]
pub async fn run_advanced_reasoning_tick_with_db(
    repo: &Path,
    run_id: &str,
    target: PortTargetRequest,
    config: AdvancedReasoningConfig,
    runtime: PortRuntimeOptions,
    fake_worker_cycle: bool,
    model_client: &dyn ModelClient,
    db: &Db,
) -> Result<AdvancedReasoningTickReport> {
    let sink = EventSink::open(repo, run_id)?;
    daemon_store::ensure_daemon_run(
        db,
        repo,
        run_id,
        daemon_store::port_spec_with_runtime(&target, &runtime),
    )?;
    emit_state(&sink, "capture_target")?;
    sink.emit(
        EventKind::RunStarted,
        json!({
            "workflow": "zyal_advanced_port",
            "target": target.target,
            "replacement": target.replacement,
        }),
    )?;

    let EarlyPhasesState {
        mut artifacts,
        mut edges,
        lanes,
        master,
        mut plan,
        evidence,
        graph,
        graph_summary,
        reduce_receipt,
        stage0_master_plan_json,
    } = run_early_phases(
        repo,
        run_id,
        db,
        &sink,
        model_client,
        &target,
        &config,
        &runtime,
    )
    .await?;

    let (_parity_seed_receipt, parity_seed_value) = complete_structured(
        repo,
        run_id,
        db,
        &sink,
        model_client,
        ModelTaskKind::ParityGenerate,
        "Generate target-switched parity seed cases from the evidence as JSON.",
    )
    .await?;

    emit_state(&sink, "track_stage")?;
    emit_state(&sink, "brainstorm_phase")?;
    emit_state(&sink, "finalize_phase_plan")?;
    let fake_task_completed = if fake_worker_cycle {
        emit_state(&sink, "build_phase")?;
        let completed = daemon_store::persist_fake_worker_pass(db, run_id, &plan)?;
        if let Some(task_id) = &completed {
            sink.emit(
                EventKind::WorkerPass,
                json!({"task_id": task_id, "worker_id": "fake-worker-advanced"}),
            )?;
        }
        completed
    } else {
        None
    };
    emit_state(&sink, "verify_phase")?;
    emit_state(&sink, "heal_integration")?;

    let mut memory_capsules = Vec::new();
    let memory = MemoryCapsule {
        id: format!("memory-{run_id}-master-plan"),
        run_id: run_id.to_string(),
        artifact_id: master.id.clone(),
        scope: "repo".to_string(),
        status: "verified".to_string(),
        summary: "Advanced port plans must be generated from current target evidence, not baked target lists."
            .to_string(),
        evidence_level: EvidenceLevel::Executable,
        confidence: 0.8,
        payload_json: json!({"source_artifact": master.id}),
        memory_kind: zyal_core::MemoryKind::Semantic,
        promotion_status: zyal_core::MemoryPromotionStatus::Scratch,
        claim_text: "Advanced port plans are generated from evidence per run, not baked target lists.".to_string(),
        approved_by_role: None,
        content_hash: stable_reasoning_hash(&json!({
            "run_id": run_id,
            "artifact_id": master.id,
            "summary": "Advanced port plans must be generated from current target evidence, not baked target lists."
        })),
    };
    daemon_store::persist_memory_capsule(db, run_id, &memory)?;
    sink.emit(
        EventKind::MemoryCapsule,
        json!({"id": memory.id, "status": memory.status}),
    )?;
    memory_capsules.push(memory);

    emit_state(&sink, "generate_parity")?;
    let cases = generate_seed_cases(&target, &evidence, &parity_seed_value);
    let parity_seed_artifact = persist_artifact(
        db,
        run_id,
        &sink,
        artifact(
            "artifact-parity-seeds",
            run_id,
            ReasoningRole::Verifier,
            ReasoningArtifactKind::ParityGap,
            "Generated parity seeds",
            "Generated Redline-style parity seed cases from bounded target evidence.",
            EvidenceLevel::Executable,
            0.8,
            json!({
                "case_ids": cases.iter().map(|case| case.id.clone()).collect::<Vec<_>>(),
                "model": parity_seed_value,
            }),
            &config,
        ),
    )?;
    edges.push(persist_edge(
        db,
        run_id,
        &master.id,
        &parity_seed_artifact.id,
        "generates_parity",
    )?);
    artifacts.push(parity_seed_artifact);
    let baseline_benchmark = if runtime.proofs.reasoning_benchmark {
        let prompt = benchmark_prompt(&target, &evidence);
        let (baseline_receipt, _baseline_value) = complete_structured(
            repo,
            run_id,
            db,
            &sink,
            model_client,
            ModelTaskKind::HardEscalation,
            &prompt,
        )
        .await?;
        Some(score_baseline(
            &prompt,
            baseline_receipt.response.as_deref().unwrap_or("{}"),
            &evidence,
            &cases,
        ))
    } else {
        None
    };
    let mut reference = FakeTargetAdapter::new("reference");
    let mut candidate = FakeTargetAdapter::new("candidate");
    let parity_report = run_target_switched_cases(&mut reference, &mut candidate, &cases)?;
    let parity_artifacts = write_report_artifacts(repo, run_id, &cases, parity_report)?;
    sink.emit(
        EventKind::ParityManifestGenerated,
        json!({"cases": cases.len(), "approved": cases.iter().filter(|case| case.is_required()).count()}),
    )?;
    let summary_text = fs::read_to_string(&parity_artifacts.summary_json)
        .with_context(|| format!("read {}", parity_artifacts.summary_json.display()))?;
    let parity_summary: crate::parity_lab::ParitySummary =
        serde_json::from_str(&summary_text).context("parse parity summary")?;
    daemon_store::persist_parity_summary(
        db,
        run_id,
        &daemon_store::target_id(run_id),
        &cases,
        &parity_artifacts,
        &parity_summary,
    )?;
    sink.emit(
        EventKind::ParityResult,
        json!({"status": parity_summary.status, "gaps": parity_summary.gaps.len()}),
    )?;

    emit_state(&sink, "close_parity_perf")?;
    if !parity_summary.gaps.is_empty() {
        sink.emit(
            EventKind::ParityGap,
            json!({"count": parity_summary.gaps.len()}),
        )?;
        let stage_id = plan
            .stages
            .last()
            .map(|stage| stage.id.clone())
            .unwrap_or_else(|| "stage-parity".to_string());
        plan.tasks.extend(
            parity_summary
                .gaps
                .iter()
                .map(|gap| crate::parity_lab::parity_gap_to_followup_task(gap, &stage_id)),
        );
        daemon_store::persist_master_plan(db, run_id, &plan)?;
    }
    let reasoning_benchmark_json = if let Some(report) = baseline_benchmark {
        let report = finish_tournament_score(report, &plan, &evidence, &cases, &artifacts);
        let path = write_benchmark_report(repo, run_id, &report)?;
        let benchmark_artifact = persist_artifact(
            db,
            run_id,
            &sink,
            artifact(
                "artifact-reasoning-benchmark",
                run_id,
                ReasoningRole::Verifier,
                ReasoningArtifactKind::ReasoningBenchmark,
                "Reasoning benchmark",
                format!(
                    "Tournament {} baseline on the hard architecture planning prompt.",
                    if report.winner == "tournament" {
                        "beat"
                    } else {
                        "did not beat"
                    }
                ),
                EvidenceLevel::Executable,
                0.9,
                serde_json::to_value(&report)?,
                &config,
            ),
        )?;
        edges.push(persist_edge(
            db,
            run_id,
            &master.id,
            &benchmark_artifact.id,
            "benchmarked_by",
        )?);
        sink.emit(
            EventKind::BenchmarkResult,
            json!({
                "winner": report.winner,
                "baseline": report.baseline_score.total,
                "tournament": report.tournament_score.total,
            }),
        )?;
        artifacts.push(benchmark_artifact);
        Some(path)
    } else {
        None
    };
    let parity_gate_passed = parity_summary.status == "passed"
        && parity_summary.gaps.is_empty()
        && parity_summary.missing_perf == 0
        && parity_summary.perf_over_budget == 0;
    let final_state = if parity_gate_passed {
        emit_state(&sink, "complete")?;
        daemon_store::mark_daemon_run(db, run_id, "complete", "complete", None)?;
        "complete"
    } else {
        emit_state(&sink, "blocked")?;
        daemon_store::mark_daemon_run(
            db,
            run_id,
            "blocked",
            "parity_gate",
            Some("parity gaps must be routed and closed before completion"),
        )?;
        "blocked"
    };

    let reasoning_graph_json = export_reasoning_graph(
        repo,
        run_id,
        &graph,
        &artifacts,
        &edges,
        &lanes,
        &memory_capsules,
    )?;

    Ok(AdvancedReasoningTickReport {
        run_id: run_id.to_string(),
        target_id: daemon_store::target_id(run_id),
        plan,
        model_receipt: reduce_receipt,
        graph_summary,
        fake_task_completed,
        advanced: AdvancedReasoningSummary {
            state: final_state.to_string(),
            artifact_count: artifacts.len(),
            lane_count: lanes.len(),
            memory_capsule_count: memory_capsules.len(),
            parity_gap_count: parity_summary.gaps.len(),
            reasoning_graph_json,
            parity_generated_manifest_json: parity_artifacts.generated_manifest_json,
            parity_approved_ci_txt: parity_artifacts.approved_ci_txt,
            parity_raw_jsonl: parity_artifacts.raw_jsonl,
            parity_summary_json: parity_artifacts.summary_json,
            parity_gaps_json: parity_artifacts.gaps_json,
            stage0_master_plan_json,
            reasoning_benchmark_json,
        },
    })
}
