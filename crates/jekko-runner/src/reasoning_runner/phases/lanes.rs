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
    artifact, complete_structured_model_only, complete_structured_recoverable, emit_state,
    flush_model_only_result, persist_artifact, persist_edge, ModelOnlyOutcome,
    StructuredCompletion,
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

fn lane_prompt(
    idx: usize,
    strategy: &str,
    evidence: &[LoadedEvidence],
    graph_slice: &str,
) -> String {
    let mut prompt = format!(
        "Blind lane {lane}: brainstorm target-derived port stages as JSON. Strategy: {strategy}. Evidence:\n{evidence}",
        lane = idx + 1,
        evidence = evidence_prompt_fragment(evidence),
    );
    if !graph_slice.is_empty() {
        prompt.push_str(
            "\nRepository blast-radius (highest-impact symbols/files and their test coverage \u{2014} weigh ripple and protect untested paths):\n",
        );
        prompt.push_str(graph_slice);
    }
    prompt
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
    graph_slice: &str,
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
            graph_slice,
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
            graph_slice,
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
    graph_slice: &str,
    cap: usize,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    for idx in 0..cap {
        let strategy = STRATEGIES[idx % STRATEGIES.len()];
        let completion = complete_structured_recoverable(
            repo,
            run_id,
            db,
            sink,
            model_client,
            ModelTaskKind::StageBrainstorm,
            &lane_prompt(idx, strategy, evidence, graph_slice),
        )
        .await?;
        let (brainstorm_value, status, confidence) =
            brainstorm_lane_payload(completion, strategy, idx);
        persist_brainstorm_lane(
            db,
            run_id,
            sink,
            config,
            context,
            idx,
            strategy,
            brainstorm_value,
            &status,
            confidence,
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
    graph_slice: &str,
    cap: usize,
    artifacts: &mut Vec<ReasoningArtifact>,
    edges: &mut Vec<ReasoningEdge>,
    lanes: &mut Vec<ReasoningLane>,
) -> Result<()> {
    let repo_path = repo.to_path_buf();
    let run_id_owned = run_id.to_string();

    let results = run_lanes_parallel(cap, |idx| {
        let strategy = STRATEGIES[idx % STRATEGIES.len()].to_string();
        let prompt = lane_prompt(idx, &strategy, evidence, graph_slice);
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
        let strategy = STRATEGIES[idx % STRATEGIES.len()];
        let completion = flush_model_only_result(
            db,
            run_id,
            sink,
            lane_result.map(|(_strategy, outcome): (String, ModelOnlyOutcome)| outcome),
        )?;
        let (brainstorm_value, status, confidence) =
            brainstorm_lane_payload(completion, strategy, idx);
        // Serialized persistence + event emission, in deterministic
        // lane-index order. SQLite stays single-writer, EventSink keeps its
        // append-only ordering, and the reducer fence holds because the next
        // phase (`critique_phase`) reads lanes through SQL after this loop
        // returns.
        persist_brainstorm_lane(
            db,
            run_id,
            sink,
            config,
            context,
            idx,
            strategy,
            brainstorm_value,
            &status,
            confidence,
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
    lane_status: &str,
    confidence: f64,
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
            confidence,
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
        status: lane_status.to_string(),
        artifact_ids: vec![proposal.id.clone()],
        write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
        worker_id: Some(format!("reasoner-{}", idx + 1)),
        confidence: proposal.confidence,
    };
    daemon_store::persist_reasoning_lane(db, run_id, &lane)?;
    sink.emit(
        EventKind::ReasoningLane,
        json!({"id": lane.id, "role": "planner", "status": lane.status}),
    )?;
    lanes.push(lane);
    artifacts.push(proposal);
    Ok(())
}

fn brainstorm_lane_payload(
    completion: StructuredCompletion,
    strategy: &str,
    idx: usize,
) -> (serde_json::Value, String, f64) {
    match completion {
        StructuredCompletion::Parsed { value, .. } => (value, "complete".to_string(), 0.5),
        StructuredCompletion::RecoveredFailure { error, .. } => (
            json!({
                "recovered_from_model_error": error,
                "strategy": strategy,
                "fallback_stage": format!("recovered-stage-{}", idx + 1),
                "proposal": "Continue with evidence-derived stage planning; defer specifics to reducer and parity gates.",
            }),
            "recovered".to_string(),
            0.2,
        ),
    }
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
    let (critique_value, confidence) = match complete_structured_recoverable(
        repo,
        run_id,
        db,
        sink,
        model_client,
        ModelTaskKind::StageCritique,
        "Critique the generic stage proposals as JSON.",
    )
    .await?
    {
        StructuredCompletion::Parsed { value, .. } => (value, 0.45),
        StructuredCompletion::RecoveredFailure { error, .. } => (
            json!({
                "recovered_from_model_error": error,
                "lane_count": lanes.len(),
                "fallback_critique": "Proceed with low-confidence recovered critique; reducer and parity gates must validate the plan.",
            }),
            0.2,
        ),
    };
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
            confidence,
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
