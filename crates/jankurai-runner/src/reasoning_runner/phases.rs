//! Early reasoning phases for [`run_advanced_reasoning_tick_with_db`].
//!
//! These are extracted from the orchestrator so each phase function stays
//! readable and the parent module stays under the audit shape threshold.
//! They share state via [`EarlyPhasesState`] which the orchestrator builds
//! incrementally; the late phases (parity, benchmark, finalization) remain
//! in the orchestrator since they cross-reference each other heavily.

mod context;
mod fanout;
mod finalize;
mod lanes;

use std::path::{Path, PathBuf};

use anyhow::Result;
use jekko_store::db::Db;

use crate::events::EventSink;
use crate::evidence::LoadedEvidence;
use crate::model_client::{ModelCallReceipt, ModelClient};
use crate::port::{PortMasterPlan, PortRuntimeOptions, PortTargetRequest};
use crate::reasoning::{AdvancedReasoningConfig, ReasoningArtifact, ReasoningEdge, ReasoningLane};
use crate::repo_graph::RepoGraph;

use self::context::{context_phase, frame_phase};
use self::finalize::{master_plan_phase, verify_phase};
use self::lanes::{brainstorm_phase, critique_phase};

/// Output of [`run_early_phases`] consumed by the orchestrator's late phases.
pub(super) struct EarlyPhasesState {
    pub artifacts: Vec<ReasoningArtifact>,
    pub edges: Vec<ReasoningEdge>,
    pub lanes: Vec<ReasoningLane>,
    pub master: ReasoningArtifact,
    pub plan: PortMasterPlan,
    pub evidence: Vec<LoadedEvidence>,
    pub graph: RepoGraph,
    pub graph_summary: serde_json::Value,
    pub reduce_receipt: ModelCallReceipt,
    pub stage0_master_plan_json: Option<PathBuf>,
}

/// Run the frame → context → brainstorm → critique → finalize-plan → verify
/// sequence and return the accumulated state.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_early_phases(
    repo: &Path,
    run_id: &str,
    db: &Db,
    sink: &EventSink,
    model_client: &dyn ModelClient,
    target: &PortTargetRequest,
    config: &AdvancedReasoningConfig,
    runtime: &PortRuntimeOptions,
) -> Result<EarlyPhasesState> {
    let mut artifacts: Vec<ReasoningArtifact> = Vec::new();
    let mut edges: Vec<ReasoningEdge> = Vec::new();
    let mut lanes: Vec<ReasoningLane> = Vec::new();

    let frame = frame_phase(repo, run_id, db, sink, model_client, target, config).await?;
    artifacts.push(frame.clone());

    let (context, evidence, graph, graph_summary) =
        context_phase(repo, run_id, db, sink, &frame, runtime, config, &mut edges).await?;
    artifacts.push(context.clone());

    brainstorm_phase(
        repo,
        run_id,
        db,
        sink,
        model_client,
        config,
        &evidence,
        &context,
        &mut artifacts,
        &mut edges,
        &mut lanes,
    )
    .await?;

    let critique = critique_phase(
        repo,
        run_id,
        db,
        sink,
        model_client,
        config,
        &lanes,
        &mut edges,
    )
    .await?;
    artifacts.push(critique.clone());

    let (master, plan, reduce_receipt, stage0_master_plan_json) = master_plan_phase(
        repo,
        run_id,
        db,
        sink,
        model_client,
        target,
        config,
        runtime,
        &evidence,
        &critique,
        &mut edges,
    )
    .await?;
    artifacts.push(master.clone());

    let verifier = verify_phase(
        repo,
        run_id,
        db,
        sink,
        model_client,
        config,
        &master,
        &mut edges,
    )
    .await?;
    artifacts.push(verifier);

    Ok(EarlyPhasesState {
        artifacts,
        edges,
        lanes,
        master,
        plan,
        evidence,
        graph,
        graph_summary,
        reduce_receipt,
        stage0_master_plan_json,
    })
}
