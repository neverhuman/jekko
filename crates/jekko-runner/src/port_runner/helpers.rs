use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::classifier;
use crate::jankurai_gate::AuditSnapshot;
use crate::port::PortTargetRequest;
use crate::repo_graph::RepoGraph;

pub(super) fn planning_prompt(target: &PortTargetRequest, graph: &RepoGraph) -> String {
    format!(
        "Draft a generic port master plan.\nTarget: {}\nReplacement: {}\nRequest: {}\nGraph summary: {:?}",
        target.target,
        target.replacement,
        target.request,
        graph.summary(),
    )
}

pub(super) fn current_audit_snapshot(repo: &Path) -> Result<AuditSnapshot> {
    let classify = classifier::classify(repo)?;
    Ok(AuditSnapshot {
        score: classify.score,
        hard_findings: classify.hard_total,
        caps: classify.caps_total,
    })
}

pub(super) fn graph_summary_json(graph: &RepoGraph) -> Result<serde_json::Value> {
    serde_json::to_value(graph.summary()).context("serialize repo graph summary")
}

pub(super) fn assert_clean_tree(repo: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .with_context(|| format!("git status in {}", repo.display()))?;
    if !output.status.success() {
        return Err(anyhow!("git status failed in {}", repo.display()));
    }
    if !output.stdout.is_empty() {
        return Err(anyhow!("working tree dirty; pass allow_dirty=true"));
    }
    Ok(())
}

#[allow(dead_code)]
fn _pathbuf_send_sync(_: PathBuf) {}
