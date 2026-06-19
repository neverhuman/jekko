//! Build a `RunSummary` by folding events.jsonl + adjacent artifacts.

use std::path::Path;

use anyhow::Result;

use super::fold;
use super::types::RunSummary;

/// Build a summary by reading the run directory's events.jsonl + adjacent
/// artifacts. Tolerant of missing/partial files - surfaces what exists.
pub fn build(run_dir: &Path) -> Result<RunSummary> {
    let run_id = run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut summary = RunSummary::empty(&run_id);
    summary.populate_artifact_paths(run_dir);
    fold::fold_run(&mut summary, run_dir)?;
    Ok(summary)
}
