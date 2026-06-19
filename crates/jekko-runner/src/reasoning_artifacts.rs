//! Artifact construction + persistence helpers for advanced reasoning.
//!
//! Split out of `reasoning_io.rs` to keep that file under the 500-LOC
//! authored-source ceiling and to expose a cleaner public surface: the
//! retry/io machinery lives next to the model-call loop, while this module
//! owns the `ReasoningArtifact` lifecycle (construct → persist → emit →
//! export).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::model_policy::ModelTaskKind;
use crate::reasoning::{
    AdvancedReasoningConfig, EvidenceLevel, MemoryCapsule, ReasoningArtifact,
    ReasoningArtifactKind, ReasoningEdge, ReasoningLane, ReasoningRole,
};
use crate::repo_graph::RepoGraph;

/// Construct a `ReasoningArtifact`, then run the storage-prep pass so
/// downstream consumers see a fully-shaped artifact (id, payload, scrubbed
/// targets, normalised confidence). The returned value is unsaved.
#[allow(clippy::too_many_arguments)]
pub(crate) fn artifact(
    id: impl Into<String>,
    run_id: &str,
    role: ReasoningRole,
    kind: ReasoningArtifactKind,
    title: impl Into<String>,
    summary: impl Into<String>,
    evidence_level: EvidenceLevel,
    confidence: f64,
    payload_json: serde_json::Value,
    config: &AdvancedReasoningConfig,
) -> ReasoningArtifact {
    let mut artifact = ReasoningArtifact::new(
        id,
        run_id,
        role,
        kind,
        title,
        summary,
        evidence_level,
        confidence,
        payload_json,
    );
    artifact.prepare_for_storage(config);
    artifact
}

/// Persist a reasoning artifact and emit the matching `ReasoningArtifact`
/// event so live watchers see it immediately.
pub(crate) fn persist_artifact(
    db: &Db,
    run_id: &str,
    sink: &EventSink,
    artifact: ReasoningArtifact,
) -> Result<ReasoningArtifact> {
    daemon_store::persist_reasoning_artifact(db, run_id, &artifact)?;
    sink.emit(
        EventKind::ReasoningArtifact,
        json!({"id": artifact.id, "kind": artifact.kind, "status": artifact.status}),
    )?;
    Ok(artifact)
}

/// Persist a directed reasoning edge (`src -> dst` with a kind label).
pub(crate) fn persist_edge(
    db: &Db,
    run_id: &str,
    src: &str,
    dst: &str,
    kind: &str,
) -> Result<ReasoningEdge> {
    let edge = ReasoningEdge {
        run_id: run_id.to_string(),
        src_artifact_id: src.to_string(),
        dst_artifact_id: dst.to_string(),
        kind: kind.to_string(),
        weight: Some(1.0),
        payload_json: json!({}),
    };
    daemon_store::persist_reasoning_edge(db, run_id, &edge)?;
    Ok(edge)
}

/// Render the reasoning DAG (artifacts + edges + lanes + memory) to a
/// `target/zyal/reasoning/<run_id>/reasoning-graph.json` JSON document
/// for downstream review/replay.
pub(crate) fn export_reasoning_graph(
    repo: &Path,
    run_id: &str,
    repo_graph: &RepoGraph,
    artifacts: &[ReasoningArtifact],
    edges: &[ReasoningEdge],
    lanes: &[ReasoningLane],
    memory_capsules: &[MemoryCapsule],
) -> Result<PathBuf> {
    let path = repo
        .join("target/zyal/reasoning")
        .join(run_id)
        .join("reasoning-graph.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let payload = json!({
        "schema_version": "zyal.reasoning.graph.v1",
        "run_id": run_id,
        "repo_graph_summary": repo_graph.summary(),
        "artifacts": artifacts,
        "edges": edges,
        "lanes": lanes,
        "memory_capsules": memory_capsules,
    });
    fs::write(&path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// Emit a `ReasoningState` event with the given state label. Centralised so
/// the call sites read the same shape.
pub(crate) fn emit_state(sink: &EventSink, state: &str) -> Result<()> {
    sink.emit(EventKind::ReasoningState, json!({"state": state}))
}

/// Synthetic structured value the fake provider returns when the test
/// fixture has no canned response. Mirrors the canonical shape of a real
/// parsed model output so downstream consumers don't special-case fake mode.
pub(crate) fn synthetic_structured_value(kind: ModelTaskKind) -> serde_json::Value {
    json!({
        "kind": format!("{kind:?}"),
        "summary": "deterministic fake structured response",
    })
}
