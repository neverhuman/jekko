use std::path::Path;

use anyhow::Result;
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::evidence::{load_evidence_inputs, LoadedEvidence};
use crate::model_client::ModelClient;
use crate::model_policy::ModelTaskKind;
use crate::port::{PortRuntimeOptions, PortTargetRequest};
use crate::reasoning::{
    AdvancedReasoningConfig, EvidenceLevel, ReasoningArtifact, ReasoningArtifactKind,
    ReasoningEdge, ReasoningRole,
};
use crate::reasoning_io::{
    artifact, complete_structured_recoverable, emit_state, persist_artifact, persist_edge,
    StructuredCompletion,
};
use crate::repo_graph::{build_repo_graph, RepoGraph};

pub(super) async fn frame_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    target: &PortTargetRequest,
    config: &AdvancedReasoningConfig,
) -> Result<ReasoningArtifact> {
    emit_state(sink, "frame_request")?;
    let frame_value = match complete_structured_recoverable(
        repo,
        run_id,
        db,
        sink,
        model_client,
        ModelTaskKind::Frame,
        &format!(
            "Frame this port request as JSON with objective and acceptance criteria: {}",
            target.request
        ),
    )
    .await?
    {
        StructuredCompletion::Parsed { value, .. } => value,
        StructuredCompletion::RecoveredFailure { error, .. } => json!({
            "recovered_from_model_error": error,
            "objective": target.request,
            "acceptance": ["derive evidence", "produce master plan", "generate parity cases"],
        }),
    };
    persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            "artifact-frame",
            run_id,
            ReasoningRole::Framer,
            ReasoningArtifactKind::TaskContract,
            "Task contract",
            format!("{} -> {}", target.target, target.replacement),
            EvidenceLevel::ExternalGrounding,
            0.55,
            json!({
                "target": target,
                "model": frame_value,
            }),
            config,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn context_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    frame: &ReasoningArtifact,
    runtime: &PortRuntimeOptions,
    config: &AdvancedReasoningConfig,
    edges: &mut Vec<ReasoningEdge>,
) -> Result<(
    ReasoningArtifact,
    Vec<LoadedEvidence>,
    RepoGraph,
    serde_json::Value,
)> {
    emit_state(sink, "retrieve_context")?;
    let graph = build_repo_graph(repo)?;
    daemon_store::persist_repo_graph(db, run_id, &graph)?;
    let evidence = load_evidence_inputs(repo, &runtime.evidence_inputs)?;
    sink.emit(
        EventKind::ReasoningArtifact,
        json!({"id": "evidence-inputs", "kind": "evidence", "count": evidence.len()}),
    )?;
    let graph_summary = serde_json::to_value(graph.summary())?;
    let context = persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            "artifact-context",
            run_id,
            ReasoningRole::Retriever,
            ReasoningArtifactKind::ContextPack,
            "Repository graph context",
            "Captured repository files, docs, tests, Rust symbols, and approximate calls.",
            EvidenceLevel::ExternalGrounding,
            0.55,
            json!({
                "graph_summary": graph_summary,
                "evidence": evidence.iter().map(LoadedEvidence::receipt).collect::<Vec<_>>(),
            }),
            config,
        ),
    )?;
    edges.push(persist_edge(
        db,
        run_id,
        &frame.id,
        &context.id,
        "context_for",
    )?);
    Ok((context, evidence, graph, graph_summary))
}
