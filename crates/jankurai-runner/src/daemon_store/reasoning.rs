use anyhow::Result;
use jekko_store::daemon::{
    self, MemoryCapsuleRow, ReasoningArtifactRow, ReasoningEdgeRow, ReasoningLaneRow,
    RepoGraphEdgeRow, RepoGraphNodeRow,
};
use jekko_store::db::Db;

use crate::reasoning::{MemoryCapsule, ReasoningArtifact, ReasoningEdge, ReasoningLane};
use crate::repo_graph::RepoGraph;

use super::helpers::{label, now_ms};

/// Persist one reasoning artifact.
pub fn persist_reasoning_artifact(
    db: &Db,
    run_id: &str,
    artifact: &ReasoningArtifact,
) -> Result<()> {
    let now = now_ms();
    daemon::upsert_reasoning_artifact(
        db.connection(),
        &ReasoningArtifactRow {
            id: artifact.id.clone(),
            run_id: run_id.to_string(),
            role: label(&artifact.role)?,
            kind: label(&artifact.kind)?,
            title: artifact.title.clone(),
            summary: artifact.summary.clone(),
            evidence_level: label(&artifact.evidence_level)?,
            confidence: artifact.confidence,
            payload_json: Some(artifact.payload_json.clone()),
            content_hash: artifact.content_hash.clone(),
            status: artifact.status.clone(),
            time_created: now,
            time_updated: now,
        },
    )?;
    Ok(())
}

/// Persist one reasoning edge.
pub fn persist_reasoning_edge(db: &Db, run_id: &str, edge: &ReasoningEdge) -> Result<()> {
    edge.validate()?;
    daemon::upsert_reasoning_edge(
        db.connection(),
        &ReasoningEdgeRow {
            run_id: run_id.to_string(),
            src_artifact_id: edge.src_artifact_id.clone(),
            dst_artifact_id: edge.dst_artifact_id.clone(),
            kind: edge.kind.clone(),
            weight: edge.weight,
            payload_json: Some(edge.payload_json.clone()),
            time_created: now_ms(),
        },
    )?;
    Ok(())
}

/// Persist one reasoning lane.
pub fn persist_reasoning_lane(db: &Db, run_id: &str, lane: &ReasoningLane) -> Result<()> {
    let now = now_ms();
    daemon::upsert_reasoning_lane(
        db.connection(),
        &ReasoningLaneRow {
            id: lane.id.clone(),
            run_id: run_id.to_string(),
            role: label(&lane.role)?,
            strategy: lane.strategy.clone(),
            status: lane.status.clone(),
            artifact_ids: lane.artifact_ids.clone(),
            write_scope: lane.write_scope.clone(),
            worker_id: lane.worker_id.clone(),
            confidence: lane.confidence,
            time_created: now,
            time_updated: now,
        },
    )?;
    Ok(())
}

/// Persist a verified or rejected memory capsule.
pub fn persist_memory_capsule(db: &Db, run_id: &str, capsule: &MemoryCapsule) -> Result<()> {
    if !capsule.can_write_permanent() {
        anyhow::bail!("memory capsule is not eligible for durable write");
    }
    let now = now_ms();
    daemon::upsert_memory_capsule(
        db.connection(),
        &MemoryCapsuleRow {
            id: capsule.id.clone(),
            run_id: run_id.to_string(),
            artifact_id: capsule.artifact_id.clone(),
            scope: capsule.scope.clone(),
            status: capsule.status.clone(),
            summary: capsule.summary.clone(),
            evidence_level: label(&capsule.evidence_level)?,
            confidence: capsule.confidence,
            payload_json: Some(capsule.payload_json.clone()),
            content_hash: capsule.content_hash.clone(),
            time_created: now,
            time_updated: now,
            memory_kind: serde_json::to_value(capsule.memory_kind)?
                .as_str()
                .unwrap_or("semantic")
                .to_string(),
            promotion_status: serde_json::to_value(capsule.promotion_status)?
                .as_str()
                .unwrap_or("scratch")
                .to_string(),
            claim_text: capsule.claim_text.clone(),
            approved_by_role: capsule.approved_by_role.clone(),
            // E2 embedding is computed by an Embedder pass over the capsule
            // summary/claim; the persist path stays embedding-agnostic so the
            // jankurai-runner orchestrator can either pre-embed (Phase E2) or
            // skip (E2 stub / cold-start runs).
            embedding: None,
        },
    )?;
    Ok(())
}

/// Persist a lightweight repository graph for the run.
pub fn persist_repo_graph(db: &Db, run_id: &str, graph: &RepoGraph) -> Result<()> {
    let conn = db.connection();
    let now = now_ms();
    for node in &graph.nodes {
        daemon::upsert_repo_graph_node(
            conn,
            &RepoGraphNodeRow {
                id: node.id.clone(),
                run_id: run_id.to_string(),
                kind: node.kind.clone(),
                key: node.key.clone(),
                label: node.label.clone(),
                payload_json: node.payload_json.clone(),
                time_created: now,
                time_updated: now,
            },
        )?;
    }
    for edge in &graph.edges {
        daemon::upsert_repo_graph_edge(
            conn,
            &RepoGraphEdgeRow {
                run_id: run_id.to_string(),
                src_node_id: edge.from.clone(),
                dst_node_id: edge.to.clone(),
                kind: edge.kind.clone(),
                payload_json: edge.payload_json.clone(),
                time_created: now,
            },
        )?;
    }
    Ok(())
}
