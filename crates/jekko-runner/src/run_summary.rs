//! GOD-level per-run summary builder + writer.
//!
//! Reads the run's events.jsonl (and optionally model_receipts.jsonl,
//! replay_receipt.json) and folds them into a single `RunSummary` struct
//! that captures everything an operator or future agent needs to know
//! about the run without re-running it. Writes `summary.json` (machine)
//! and `summary.md` (human) into the run directory.
//!
//! Schema version: `zyal.run_summary.v1`.
//!
//! See `docs/ZYAL/AGENT_PLAYBOOK.md` for field-by-field interpretation.

mod build;
mod fold;
mod fold_support;
#[cfg(test)]
mod tests;
mod types;
mod write;

pub use build::build;
pub use types::{HaltReason, PipelineProgress, RunSummary, SignalRow, SCHEMA_VERSION};
pub use write::{render_markdown, write_summary};

use std::path::Path;

use anyhow::Result;

/// Build the summary from a run dir AND write `summary.{json,md}` next to
/// the events file. Idempotent — overwrites any prior summary.
pub fn build_and_write(run_dir: &Path) -> Result<RunSummary> {
    let summary = build(run_dir)?;
    write_summary(run_dir, &summary)?;
    Ok(summary)
}
