use std::path::{Path, PathBuf};

use anyhow::Result;
use jekko_store::db::Db;
use serde_json::{json, Value};

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::evidence::LoadedEvidence;
use crate::model_client::{ModelCallReceipt, ModelClient};
use crate::model_policy::ModelTaskKind;
use crate::port::{
    draft_master_plan, validate_master_plan_contract, PortMasterPlan, PortRuntimeOptions,
    PortTargetRequest,
};
use crate::reasoning::{
    AdvancedReasoningConfig, EvidenceLevel, ReasoningArtifact, ReasoningArtifactKind,
    ReasoningEdge, ReasoningRole,
};
use crate::reasoning_io::{
    artifact, complete_structured_recoverable, emit_state, persist_artifact, persist_edge,
    StructuredCompletion,
};
use crate::stage0_proof::{
    build_stage0_master_plan, evidence_prompt_fragment, parse_model_master_plan,
    write_stage0_master_plan,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn master_plan_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    target: &PortTargetRequest,
    config: &AdvancedReasoningConfig,
    runtime: &PortRuntimeOptions,
    evidence: &[LoadedEvidence],
    critique: &ReasoningArtifact,
    edges: &mut Vec<ReasoningEdge>,
) -> Result<(
    ReasoningArtifact,
    PortMasterPlan,
    ModelCallReceipt,
    Option<PathBuf>,
)> {
    emit_state(sink, "finalize_master_plan")?;
    let completion = complete_structured_recoverable(
        repo,
        run_id,
        db,
        sink,
        model_client,
        ModelTaskKind::StageReduce,
        &format!(
            "Reduce the stage proposals into a final master plan JSON. Return stages and tasks with ids, names, objectives, write scopes, and proof lanes. Evidence:\n{}",
            evidence_prompt_fragment(evidence),
        ),
    )
    .await?;
    let evidence_plan = if runtime.proofs.redis_jedis_stage0 || !evidence.is_empty() {
        Some(build_stage0_master_plan(target.clone(), evidence))
    } else {
        None
    };
    let recovery_plan = || {
        evidence_plan
            .clone()
            .unwrap_or_else(|| draft_master_plan(target.clone()))
    };
    let (reduce_receipt, reduce_value, plan) = match completion {
        StructuredCompletion::Parsed { receipt, value } => {
            let (reduce_value, plan) = if receipt.provider == "fake" {
                (value, recovery_plan())
            } else {
                match parse_model_master_plan(target.clone(), &value) {
                    Ok(plan) => (value, plan),
                    Err(err) => (
                        json!({
                            "recovered_from_model_error": format!(
                                "reducer master plan validation failed: {err}"
                            ),
                            "model": value,
                        }),
                        recovery_plan(),
                    ),
                }
            };
            (receipt, reduce_value, plan)
        }
        StructuredCompletion::RecoveredFailure { receipt, error, .. } => {
            let receipt = receipt.unwrap_or_else(|| {
                ModelCallReceipt::failure(
                    ModelTaskKind::StageReduce,
                    "recovered",
                    "recovery",
                    &error,
                )
            });
            (
                receipt,
                json!({"recovered_from_model_error": error}),
                recovery_plan(),
            )
        }
    };
    validate_master_plan_contract(&plan)?;
    daemon_store::persist_master_plan(db, run_id, &plan)?;
    let stage0_master_plan_json = if runtime.proofs.redis_jedis_stage0 {
        Some(write_stage0_master_plan(
            repo,
            run_id,
            evidence_plan.as_ref().unwrap_or(&plan),
            evidence,
        )?)
    } else {
        None
    };
    let master = persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            "artifact-master-plan",
            run_id,
            ReasoningRole::Reducer,
            ReasoningArtifactKind::MasterPlan,
            "Final master plan",
            "Reduced a generic staged master plan without target-specific hard-coded stages.",
            EvidenceLevel::Executable,
            0.8,
            json!({"plan": plan, "model": reduce_value}),
            config,
        ),
    )?;
    edges.push(persist_edge(
        db,
        run_id,
        &critique.id,
        &master.id,
        "reduced_into",
    )?);
    sink.emit(
        EventKind::PhaseFinalized,
        json!({"stage_count": plan.stages.len(), "task_count": plan.tasks.len()}),
    )?;
    Ok((master, plan, reduce_receipt, stage0_master_plan_json))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn verify_phase(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    config: &AdvancedReasoningConfig,
    master: &ReasoningArtifact,
    edges: &mut Vec<ReasoningEdge>,
) -> Result<ReasoningArtifact> {
    // Tightened schema: pass only the reduced plan payload and demand a
    // compact JSON object. This keeps verifier calls focused and parseable.
    let prompt = verifier_prompt(master);
    let (verifier_value, confidence) = match complete_structured_recoverable(
        repo,
        run_id,
        db,
        sink,
        model_client,
        ModelTaskKind::Verifier,
        &prompt,
    )
    .await?
    {
        StructuredCompletion::Parsed { value, .. } => (value, 0.8),
        StructuredCompletion::RecoveredFailure { error, .. } => (
            json!({
                "recovered_from_model_error": error,
                "verdict": "degraded",
                "requires_parity_gate": true,
            }),
            0.2,
        ),
    };
    let verifier = persist_artifact(
        db,
        run_id,
        sink,
        artifact(
            "artifact-master-plan-verifier",
            run_id,
            ReasoningRole::Verifier,
            ReasoningArtifactKind::VerificationReceipt,
            "Master plan verifier",
            "Checked the master plan for evidence coverage, unsupported claims, and parity proof hooks.",
            EvidenceLevel::Executable,
            confidence,
            json!({"model": verifier_value}),
            config,
        ),
    )?;
    edges.push(persist_edge(
        db,
        run_id,
        &master.id,
        &verifier.id,
        "verified_by",
    )?);
    Ok(verifier)
}

fn verifier_prompt(master: &ReasoningArtifact) -> String {
    let reduced = master
        .payload_json
        .get("plan")
        .map(reduced_plan_payload)
        .unwrap_or_else(|| {
            json!({
                "artifact_id": &master.id,
                "summary": &master.summary,
            })
        });
    let payload = serde_json::to_string(&reduced).unwrap_or_else(|_| "{}".to_string());
    format!(
        "Verify the reduced master plan payload against the bounded evidence already supplied. \
         Payload: {payload}\n\
         Return ONLY one compact JSON object with these exact keys: \
         {{\"accepted\":string[],\"rejected\":string[],\"unsupported\":string[],\"confidence\":number}}. \
         No prose, no markdown, no code fences."
    )
}

fn reduced_plan_payload(plan: &Value) -> Value {
    json!({
        "stages": plan.get("stages").cloned().unwrap_or(Value::Null),
        "tasks": plan.get("tasks").cloned().unwrap_or(Value::Null),
    })
}
