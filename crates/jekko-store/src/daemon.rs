//! Daemon runtime tables.
//!
//! Ported from `packages/jekko/src/session/daemon.sql.ts`. All JSON columns
//! are treated as opaque [`serde_json::Value`] so the caller can decode into
//! their domain-specific shape at use-site.
//!
//! The implementation is split per-table under [`daemon`](self) submodules
//! (each module owns one row struct and its CRUD). All public types and
//! helpers are re-exported here so the original `jekko_store::daemon::*`
//! import paths continue to work unchanged.

pub mod artifact;
pub mod event;
pub mod forever;
pub mod iteration;
pub mod port;
pub mod reasoning;
pub mod run;
mod support;
pub mod task;
pub mod task_memory;
pub mod task_pass;
pub mod worker;

pub use artifact::{upsert_artifact, DaemonArtifactRow};
pub use event::{insert_event, list_events_for_run, DaemonEventRow};
pub use forever::{
    get_concept, get_finding, list_active_concepts_for_run, list_concept_links_for_run,
    list_finding_batches_for_run, list_finding_edges_for_run, list_findings_for_run,
    list_regression_cycles_for_run, upsert_concept, upsert_concept_link, upsert_finding,
    upsert_finding_batch, upsert_finding_edge, upsert_regression_cycle, DaemonConceptLinkRow,
    DaemonConceptRow, DaemonFindingBatchRow, DaemonFindingEdgeRow, DaemonFindingRow,
    DaemonRegressionCycleRow,
};
pub use iteration::{get_iteration, upsert_iteration, DaemonIterationRow};
pub use port::{
    get_port_target, get_port_task, insert_parity_result, list_model_outcomes_for_run,
    list_parity_cases_for_target, list_parity_results_for_run, list_parity_runs_for_target,
    list_port_phases_for_target, list_port_targets_for_run, list_port_tasks_for_phase,
    list_repo_graph_edges_for_run, list_repo_graph_nodes_for_run, upsert_model_outcome,
    upsert_parity_case, upsert_parity_run, upsert_perf_budget, upsert_port_phase,
    upsert_port_target, upsert_port_task, upsert_repo_graph_edge, upsert_repo_graph_node,
    ModelOutcomeRow, ParityCaseRow, ParityResultRow, ParityRunRow, PerfBudgetRow, PortPhaseRow,
    PortTargetRow, PortTaskRow, RepoGraphEdgeRow, RepoGraphNodeRow,
};
pub use reasoning::{
    get_model_reliability, list_memory_capsules_for_run, list_model_reliability,
    list_promoted_capsules, list_reasoning_artifacts_for_run, list_reasoning_edges_for_run,
    list_reasoning_lanes_for_run, record_model_reliability_outcome, upsert_memory_capsule,
    upsert_model_reliability, upsert_reasoning_artifact, upsert_reasoning_edge,
    upsert_reasoning_lane, MemoryCapsuleRow, ModelReliabilityRow, ReasoningArtifactRow,
    ReasoningEdgeRow, ReasoningLaneRow,
};
pub use run::{delete_run, get_run, list_runs, upsert_run, DaemonRunRow};
pub use task::{delete_task, get_task, upsert_task, DaemonTaskRow};
pub use task_memory::{upsert_task_memory, DaemonTaskMemoryRow};
pub use task_pass::{get_task_pass, upsert_task_pass, DaemonTaskPassRow};
pub use worker::{upsert_worker, DaemonWorkerRow};
