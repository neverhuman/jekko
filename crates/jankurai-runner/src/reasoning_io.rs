//! I/O helpers for advanced reasoning runs.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::model_client::{ModelCallReceipt, ModelClient};
use crate::model_policy::ModelTaskKind;
use crate::reasoning::{
    AdvancedReasoningConfig, EvidenceLevel, MemoryCapsule, ReasoningArtifact,
    ReasoningArtifactKind, ReasoningEdge, ReasoningLane, ReasoningRole,
};
use crate::repo_graph::RepoGraph;

/// Outcome of the pure model-call half of [`complete_structured`].
///
/// The retry loop runs end-to-end without touching `Db` or [`EventSink`], so
/// callers can drive multiple of these concurrently (e.g. via
/// `futures::future::join_all`). The caller is responsible for replaying
/// `queued_events` through an [`EventSink`] and persisting `receipt` serially
/// after each lane joins, preserving the existing single-writer SQL
/// discipline.
#[derive(Debug)]
pub(crate) struct ModelOnlyOutcome {
    /// Final receipt (the last attempt the loop saw before returning).
    pub receipt: ModelCallReceipt,
    /// Parsed JSON value (real or synthetic for the fake provider).
    pub value: serde_json::Value,
    /// Intermediate events to emit on the awaiting task (in order).
    pub queued_events: Vec<(EventKind, serde_json::Value)>,
    /// Per-attempt receipts that need to be persisted before the final one.
    /// Empty in the common 1-attempt case.
    pub intermediate_receipts: Vec<ModelCallReceipt>,
}

/// Pure model-call retry loop. Performs no `Db`/`EventSink` I/O; instead it
/// accumulates events and intermediate receipts into the returned outcome so
/// the caller can replay them serially after joining concurrent lanes.
///
/// The borrowed `model_client` keeps this helper compatible with the existing
/// `&dyn ModelClient` orchestrator signature; concurrent fanout polls these
/// futures on the same task via `futures::future::join_all` rather than
/// `tokio::spawn`, so we don't need `Send + 'static`.
pub(crate) async fn complete_structured_model_only(
    repo: PathBuf,
    _run_id: String,
    model_client: &dyn ModelClient,
    kind: ModelTaskKind,
    prompt: String,
) -> Result<ModelOnlyOutcome> {
    let mut queued_events: Vec<(EventKind, serde_json::Value)> = Vec::new();
    let mut intermediate_receipts: Vec<ModelCallReceipt> = Vec::new();
    let mut last_error: Option<String> = None;
    for attempt in 1..=3 {
        queued_events.push((
            EventKind::ModelAttempt,
            json!({
                "kind": crate::model_client::kind_label(kind),
                "attempt": attempt,
            }),
        ));
        let receipt = model_client.complete(kind, &prompt, &repo).await?;
        if receipt.budget_used.is_some() || receipt.budget_remaining.is_some() {
            queued_events.push((
                EventKind::LiveBudget,
                json!({
                    "used": receipt.budget_used.unwrap_or(0),
                    "remaining": receipt.budget_remaining.unwrap_or(0),
                }),
            ));
        }
        if !receipt.success {
            let error = match receipt.error.clone() {
                Some(error) => error,
                None => "unknown model failure".to_string(),
            };
            push_model_attempt_outcome(&mut queued_events, &receipt, attempt, "model_failure");
            // Mark-blocked is a Db side-effect; convert into a structured error
            // the caller can act on after persisting this final receipt.
            return Err(anyhow!(ModelOnlyError::ModelFailure {
                outcome: Box::new(ModelOnlyOutcome {
                    receipt: receipt.clone(),
                    value: serde_json::Value::Null,
                    queued_events,
                    intermediate_receipts,
                }),
                error,
            }));
        }
        let Some(text) = receipt.response.as_deref() else {
            if receipt.provider == "fake" {
                push_model_attempt_outcome(
                    &mut queued_events,
                    &receipt,
                    attempt,
                    "fake_provider_synthetic_response",
                );
                return Ok(ModelOnlyOutcome {
                    receipt,
                    value: synthetic_structured_value(kind),
                    queued_events,
                    intermediate_receipts,
                });
            }
            push_model_attempt_outcome(&mut queued_events, &receipt, attempt, "missing_response");
            last_error = Some("model response missing".to_string());
            intermediate_receipts.push(receipt);
            continue;
        };
        match parse_structured_model_json(text) {
            Ok(value) => {
                push_model_attempt_outcome(&mut queued_events, &receipt, attempt, "parsed");
                push_model_outcome(&mut queued_events, &receipt, attempt, "parsed");
                return Ok(ModelOnlyOutcome {
                    receipt,
                    value,
                    queued_events,
                    intermediate_receipts,
                });
            }
            Err(_err) if receipt.provider == "fake" => {
                push_model_attempt_outcome(
                    &mut queued_events,
                    &receipt,
                    attempt,
                    "fake_provider_synthetic_response",
                );
                return Ok(ModelOnlyOutcome {
                    receipt,
                    value: synthetic_structured_value(kind),
                    queued_events,
                    intermediate_receipts,
                });
            }
            Err(err) => {
                push_model_attempt_outcome(
                    &mut queued_events,
                    &receipt,
                    attempt,
                    "retryable_failure",
                );
                last_error = Some(err.to_string());
                intermediate_receipts.push(receipt);
            }
        }
    }
    let error = match last_error {
        Some(error) => error,
        None => "invalid model JSON".to_string(),
    };
    // Use a placeholder receipt — the parse-error path historically returned
    // an Err before producing one; intermediate receipts hold the per-attempt
    // history the caller needs to persist + the blocked marker reason.
    Err(anyhow!(ModelOnlyError::ParseFailure {
        intermediate_receipts,
        queued_events,
        error,
    }))
}

/// Structured error variants surfaced by [`complete_structured_model_only`].
/// Callers downcast via `anyhow::Error::downcast_ref` to recover queued
/// events that still need to be flushed to disk before the blocked marker.
#[derive(Debug)]
pub(crate) enum ModelOnlyError {
    ModelFailure {
        outcome: Box<ModelOnlyOutcome>,
        error: String,
    },
    ParseFailure {
        intermediate_receipts: Vec<ModelCallReceipt>,
        queued_events: Vec<(EventKind, serde_json::Value)>,
        error: String,
    },
}

impl std::fmt::Display for ModelOnlyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelOnlyError::ModelFailure { error, .. } => {
                write!(f, "model call failed: {error}")
            }
            ModelOnlyError::ParseFailure { error, .. } => {
                write!(f, "advanced reasoning model JSON parse failed: {error}")
            }
        }
    }
}

impl std::error::Error for ModelOnlyError {}

pub(crate) async fn complete_structured(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    kind: ModelTaskKind,
    prompt: &str,
) -> Result<(ModelCallReceipt, serde_json::Value)> {
    let result = complete_structured_model_only(
        repo.to_path_buf(),
        run_id.to_string(),
        model_client,
        kind,
        prompt.to_string(),
    )
    .await;
    match result {
        Ok(outcome) => {
            for receipt in &outcome.intermediate_receipts {
                daemon_store::persist_model_receipt(db, run_id, receipt)?;
            }
            daemon_store::persist_model_receipt(db, run_id, &outcome.receipt)?;
            for (event_kind, payload) in &outcome.queued_events {
                sink.emit(*event_kind, payload.clone())?;
            }
            Ok((outcome.receipt, outcome.value))
        }
        Err(err) => match err.downcast::<ModelOnlyError>() {
            Ok(ModelOnlyError::ModelFailure { outcome, error }) => {
                for receipt in &outcome.intermediate_receipts {
                    daemon_store::persist_model_receipt(db, run_id, receipt)?;
                }
                daemon_store::persist_model_receipt(db, run_id, &outcome.receipt)?;
                for (event_kind, payload) in &outcome.queued_events {
                    sink.emit(*event_kind, payload.clone())?;
                }
                daemon_store::mark_daemon_run(
                    db,
                    run_id,
                    "blocked",
                    &outcome.receipt.kind,
                    Some(&error),
                )?;
                Err(anyhow!("model call failed: {error}"))
            }
            Ok(ModelOnlyError::ParseFailure {
                intermediate_receipts,
                queued_events,
                error,
            }) => {
                for receipt in &intermediate_receipts {
                    daemon_store::persist_model_receipt(db, run_id, receipt)?;
                }
                for (event_kind, payload) in &queued_events {
                    sink.emit(*event_kind, payload.clone())?;
                }
                mark_blocked_for_parse_error(db, run_id, &error)?;
                Err(anyhow!(
                    "advanced reasoning model JSON parse failed: {error}"
                ))
            }
            Err(other) => Err(other),
        },
    }
}

fn push_model_attempt_outcome(
    queued: &mut Vec<(EventKind, serde_json::Value)>,
    receipt: &ModelCallReceipt,
    attempt: usize,
    state: &str,
) {
    queued.push((
        EventKind::ModelAttemptOutcome,
        model_event_payload(receipt, attempt, state),
    ));
}

fn push_model_outcome(
    queued: &mut Vec<(EventKind, serde_json::Value)>,
    receipt: &ModelCallReceipt,
    attempt: usize,
    state: &str,
) {
    queued.push((
        EventKind::ModelOutcome,
        model_event_payload(receipt, attempt, state),
    ));
}

fn model_event_payload(
    receipt: &ModelCallReceipt,
    attempt: usize,
    state: &str,
) -> serde_json::Value {
    let response_bytes = match receipt.response.as_deref() {
        Some(response) => response.len(),
        None => 0,
    };
    let retry_count = match receipt.retry_count {
        Some(retry_count) => retry_count,
        None => attempt.saturating_sub(1),
    };
    let budget_used = receipt.budget_used.unwrap_or_default();
    let budget_remaining = receipt.budget_remaining.unwrap_or_default();
    json!({
        "kind": receipt.kind,
        "provider": receipt.provider,
        "model": receipt.model,
        "success": receipt.success,
        "attempt": attempt,
        "state": state,
        "latency_ms": receipt.latency_ms,
        "response_bytes": response_bytes,
        "credential_policy": receipt.credential_policy,
        "selected_credential_user_id": receipt.selected_credential_user_id,
        "credential_user_id": receipt.credential_user_id,
        "retry_count": retry_count,
        "budget_used": budget_used,
        "budget_remaining": budget_remaining,
    })
}

pub(crate) fn parse_structured_model_json(text: &str) -> serde_json::Result<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => Ok(value),
        Err(primary) => {
            for (start, ch) in text.char_indices() {
                if !matches!(ch, '{' | '[') {
                    continue;
                }
                let Some(end) = find_balanced_json_end(text, start) else {
                    continue;
                };
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
                    return Ok(value);
                }
            }
            Err(primary)
        }
    }
}

fn find_balanced_json_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        let idx = start + offset;
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

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

pub(crate) fn emit_state(sink: &EventSink, state: &str) -> Result<()> {
    sink.emit(EventKind::ReasoningState, json!({"state": state}))
}

fn synthetic_structured_value(kind: ModelTaskKind) -> serde_json::Value {
    json!({
        "kind": format!("{kind:?}"),
        "summary": "deterministic fake structured response",
    })
}

fn mark_blocked_for_parse_error(db: &Db, run_id: &str, error: &str) -> Result<()> {
    daemon_store::mark_daemon_run(db, run_id, "blocked", "model_json_parse", Some(error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_structured_model_json;

    #[test]
    fn parse_structured_model_json_accepts_wrapped_object() {
        let text =
            "Here is the JSON: {\"answer\":true,\"count\":2}\nExtra notes: ignore this {not json}";
        let value = parse_structured_model_json(text).expect("wrapped JSON should parse");
        assert_eq!(value["answer"], true);
        assert_eq!(value["count"], 2);
    }
}
