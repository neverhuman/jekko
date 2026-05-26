//! Headless jankurai forever-runner. Drives `jankurai audit` to zero by
//! classifying findings, scheduling parallel waves through a path-overlap DAG,
//! running work in isolated git worktrees, committing on green, rolling back
//! on red, and emitting an NDJSON event stream.
//!
//! At PR3 the crate ships standalone — no daemon-TS bridge yet. The
//! `runner::tick` loop is fully orchestrable in dry-run mode for tests; the
//! daemon-side glue lands in PR4 by tailing `target/zyal/runner-events.jsonl`.

pub mod bootstrap_check;
pub mod classifier;
pub mod commit;
pub mod daemon_store;
pub mod dag;
pub mod events;
pub mod evidence;
pub mod hashing;
pub mod hero_judge;
pub mod hero_judge_eval;
pub(crate) mod hero_judge_eval_io;
pub(crate) mod hero_judge_eval_metrics;
pub mod hero_judge_runner;
pub(crate) mod hero_judge_runner_artifacts;
pub(crate) mod hero_judge_runner_completion;
pub(crate) mod hero_judge_runner_finalize;
pub(crate) mod hero_judge_runner_flow;
pub(crate) mod hero_judge_runner_helpers;
pub mod hero_judge_search;
pub mod jankurai_gate;
pub mod locks;
pub mod memory;
pub mod model_client;
pub mod model_policy;
pub mod parity_lab;
pub mod port;
pub mod port_runner;
pub mod reasoning;
pub mod reasoning_benchmark;
pub mod reasoning_io;
pub mod reasoning_runner;
pub mod receipts;
pub mod repo_graph;
pub mod rollback;
pub mod runner;
pub mod stage0_proof;
pub mod superreasoning;
pub mod watcher;
pub mod worker_pool;
pub mod worktree;
