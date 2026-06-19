//! Serde default-builder helpers for the superreasoning policy structs.
//!
//! These small fns back the `#[serde(default = "...")]` attributes on
//! [`super::SuperReasoningConfig`] and its sibling policy structs. They are
//! kept private to the `config` module tree.

use super::{DEFAULT_SUPER_STAGE_TARGET, MAX_SUPERREASONING_WORKERS};

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_max_workers() -> usize {
    MAX_SUPERREASONING_WORKERS
}

pub(super) fn default_macro_stage_target() -> usize {
    DEFAULT_SUPER_STAGE_TARGET
}

pub(super) fn default_max_parallel_phases() -> usize {
    3
}

pub(super) fn default_per_phase_worker_cap() -> usize {
    MAX_SUPERREASONING_WORKERS
}

pub(super) fn default_memory_write_requires() -> Vec<String> {
    vec![
        "verified_or_rejected_status".to_string(),
        "source_artifact_hash".to_string(),
        "verifier_or_reducer_approval".to_string(),
        "no_raw_chain_of_thought".to_string(),
    ]
}

pub(super) fn default_context_tokens() -> usize {
    24_000
}

pub(super) fn default_promotion_threshold() -> f64 {
    0.75
}

pub(super) fn default_retention_horizon() -> String {
    "7d".to_string()
}

pub(super) fn default_graph_store() -> String {
    "sqlite".to_string()
}

pub(super) fn default_graph_slice_node_budget() -> usize {
    256
}

pub(super) fn default_required_case_tags() -> Vec<String> {
    vec!["required".to_string(), "approved".to_string()]
}

pub(super) fn default_gap_task_prefix() -> String {
    "parity-gap".to_string()
}

pub(super) fn default_p95_ratio() -> f64 {
    1.25
}

pub(super) fn default_ramdisk_root() -> String {
    "/dev/shm/zyal".to_string()
}

pub(super) fn default_run_root() -> String {
    ".zyal/runs/${run.id}".to_string()
}

pub(super) fn default_worktree_root() -> String {
    ".zyal/worktrees/${run.id}".to_string()
}

pub(super) fn default_worktree_pool_size() -> usize {
    MAX_SUPERREASONING_WORKERS
}

pub(super) fn default_gc_after() -> String {
    "14d".to_string()
}
