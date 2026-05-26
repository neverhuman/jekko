use std::path::Path;

use anyhow::Result;
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::evidence::LoadedEvidence;
use crate::model_client::ModelClient;
use crate::model_policy::ModelTaskKind;
use crate::reasoning::{
    AdvancedReasoningConfig, EvidenceLevel, ReasoningArtifact, ReasoningArtifactKind,
    ReasoningEdge, ReasoningLane, ReasoningRole,
};
use crate::reasoning_io::{
    artifact, complete_structured, complete_structured_model_only, emit_state, persist_artifact,
    persist_edge, ModelOnlyOutcome,
};
use crate::stage0_proof::evidence_prompt_fragment;

use super::fanout::run_lanes_parallel;

const STRATEGIES: &[&str] = &[
    "minimal_contract",
    "test_first",
    "protocol_surface",
    "perf_first",
    "integration_healing",
    "adversarial_gap",
    "docs_examples",
    "compatibility_matrix",
    "rollback_safety",
    "parity_lab",
];

fn lane_prompt(idx: usize, strategy: &str, evidence: &[LoadedEvidence]) -> String {
    format!(
        "Blind lane {lane}: brainstorm target-derived port stages as JSON. Strategy: {strategy}. Evidence:\n{evidence}",
        lane = idx + 1,
        evidence = evidence_prompt_fragment(evidence),
    )
}

fn parallel_brainstorm_enabled() -> bool {
    std::env::var("JEKKO_REASONING_PARALLEL").as_deref() == Ok("1")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn brainstorm_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    config: &AdvancedReasoningConfig,
    evidence: &[LoadedEvidence],
    context: &ReasoningArtifact,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    emit_state(sink, "brainstorm_stages")?;
    let cap = config.effective_worker_cap();

    if parallel_brainstorm_enabled() {
        run_brainstorm_parallel(
            repo,
            run_id,
            db,
            sink,
            model_client,
            config,
            evidence,
            context,
            cap,
            artifacts,
            edges,
            lanes,
        )
        .await
    } else {
        run_brainstorm_sequential(
            repo,
            run_id,
            db,
            sink,
            model_client,
            config,
            evidence,
            context,
            cap,
            artifacts,
            edges,
            lanes,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_brainstorm_sequential(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    config: &AdvancedReasoningConfig,
    evidence: &[LoadedEvidence],
    context: &ReasoningArtifact,
    cap: usize,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    for idx in 0..cap {
        let strategy = STRATEGIES[idx % STRATEGIES.len()];
        let (_brainstorm_receipt, brainstorm_value) = complete_structured(
            repo,
            run_id,
            db,
            sink,
            model_client,
            ModelTaskKind::StageBrainstorm,
            &lane_prompt(idx, strategy, evidence),
        )
        .await?;
        persist_brainstorm_lane(
            db,
            run_id,
            sink,
            config,
            context,
            idx,
            strategy,
            brainstorm_value,
            artifacts,
            edges,
            lanes,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_brainstorm_parallel(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    config: &AdvancedReasoningConfig,
    evidence: &[LoadedEvidence],
    context: &ReasoningArtifact,
    cap: usize,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    let repo_path = repo.to_path_buf();
    let run_id_owned = run_id.to_string();

    let results = run_lanes_parallel(cap, |idx| {
        let strategy = STRATEGIES[idx % STRATEGIES.len()].to_string();
        let prompt = lane_prompt(idx, &strategy, evidence);
        let repo_clone = repo_path.clone();
        let run_id_clone = run_id_owned.clone();
        async move {
            let outcome = complete_structured_model_only(
                repo_clone,
                run_id_clone,
                model_client,
                ModelTaskKind::StageBrainstorm,
                prompt,
            )
            .await?;
            Ok((strategy, outcome))
        }
    })
    .await;

    for (idx, lane_result) in results {
        let (strategy, outcome): (String, ModelOnlyOutcome) = lane_result?;
        // Serialized persistence + event emission, in deterministic
        // lane-index order. SQLite stays single-writer, EventSink keeps its
        // append-only ordering, and the reducer fence holds because the next
        // phase (`critique_phase`) reads lanes through SQL after this loop
        // returns.
        for receipt in &outcome.intermediate_receipts {
            daemon_store::persist_model_receipt(db, run_id, receipt)?;
        }
        daemon_store::persist_model_receipt(db, run_id, &outcome.receipt)?;
        for (event_kind, payload) in &outcome.queued_events {
            sink.emit(*event_kind, payload.clone())?;
        }
        persist_brainstorm_lane(
            db,
            run_id,
            sink,
            config,
            context,
            idx,
            &strategy,
            outcome.value,
            artifacts,
            edges,
            lanes,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_brainstorm_lane(
    db: &Db,
    run_id: &str,
    sink: &EventSink,
    config: &AdvancedReasoningConfig,
    context: &ReasoningArtifact,
    idx: usize,
    strategy: &str,
    brainstorm_value: serde_json::Value,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    let proposal = persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            format!("artifact-stage-proposal-{}", idx + 1),
            run_id,
            ReasoningRole::Planner,
            ReasoningArtifactKind::StageProposal,
            format!("Stage proposal {}", idx + 1),
            format!("Blind lane using {strategy} strategy."),
            EvidenceLevel::IndependentAgreement,
            0.5,
            json!({"strategy": strategy, "model": brainstorm_value}),
            config,
        ),
    )?;
    edges.push(persist_edge(
        db,
        run_id,
        &context.id,
        &proposal.id,
        "derived_from",
    )?);
    let lane = ReasoningLane {
        id: format!("lane-{}", idx + 1),
        run_id: run_id.to_string(),
        role: ReasoningRole::Planner,
        strategy: strategy.to_string(),
        status: "complete".to_string(),
        artifact_ids: vec![proposal.id.clone()],
        write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
        worker_id: Some(format!("reasoner-{}", idx + 1)),
        confidence: proposal.confidence,
    };
    daemon_store::persist_reasoning_lane(db, run_id, &lane)?;
    sink.emit(
        EventKind::ReasoningLane,
        json!({"id": lane.id, "role": "planner", "status": "complete"}),
    )?;
    lanes.push(lane);
    artifacts.push(proposal);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn critique_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    config: &AdvancedReasoningConfig,
    lanes: &[ReasoningLane],
    edges: &mut Vec<ReasoningEdge>,
) -> Result<ReasoningArtifact> {
    emit_state(sink, "critique_stages")?;
    // Belt-and-suspenders reducer fence: by the time we get here, the
    // brainstorm phase must have flushed every lane (sequential or
    // parallel-then-serial). If `lanes` is empty we know something upstream
    // skipped persistence — fail fast in debug builds.
    debug_assert!(
        !lanes.is_empty() || config.effective_worker_cap() == 0,
        "critique_phase invoked with no persisted brainstorm lanes; brainstorm reducer fence violated",
    );
    let (_critique_receipt, critique_value) = complete_structured(
        repo,
        run_id,
        db,
        sink,
        model_client,
        ModelTaskKind::StageCritique,
        "Critique the generic stage proposals as JSON.",
    )
    .await?;
    let critique = persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            "artifact-stage-critique",
            run_id,
            ReasoningRole::Critic,
            ReasoningArtifactKind::Critique,
            "Stage critique",
            "Critiqued stage proposals for missing evidence, overlap, and target hard-coding.",
            EvidenceLevel::IndependentAgreement,
            0.45,
            json!({"model": critique_value}),
            config,
        ),
    )?;
    for lane in lanes {
        if let Some(source) = lane.artifact_ids.first() {
            edges.push(persist_edge(
                db,
                run_id,
                source,
                &critique.id,
                "critiqued_by",
            )?);
        }
    }
    Ok(critique)
}
