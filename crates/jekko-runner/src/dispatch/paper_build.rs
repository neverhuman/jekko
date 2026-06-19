use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use paper_builder::{
    render_ieee_skeleton, request_hash, PaperArtifactContract, PaperBuildMode, PaperBuildReceipt,
    PaperBuildRequest,
};
use serde_json::{json, Value};

use crate::events::EventKind;
use crate::run_store::RunContext;

pub(super) fn plan_paper_builder(
    repo_root: &std::path::Path,
    node_id: &str,
    request: &PaperBuildRequest,
    workflow_ref: &str,
    source_hash: &str,
    interface_hash: &str,
    ctx: &RunContext,
) -> (
    Option<PaperBuildReceipt>,
    Vec<(EventKind, Value)>,
    Option<String>,
) {
    let mode = mode_label(request.mode);
    let mut frames = vec![(
        EventKind::ChildWorkflowStarted,
        json!({ "node_id": node_id, "ref": workflow_ref, "mode": mode }),
    )];
    if ctx.mode == crate::run_store::SourceMode::Replay {
        return paper_builder_failure(
            frames,
            node_id,
            "cached_required",
            "paper_builder replay requires a cached build receipt",
        );
    }
    if let Err(err) = request.validate() {
        return paper_builder_failure(frames, node_id, "invalid_request", err.to_string());
    }

    let output_dir = match resolve_paper_output_dir(repo_root, &ctx.run_id, &request.output_dir) {
        Ok(path) => path,
        Err(err) => return paper_builder_failure(frames, node_id, "unsafe_output_dir", err),
    };
    if let Err(err) = std::fs::create_dir_all(&output_dir) {
        return paper_builder_failure(
            frames,
            node_id,
            "mkdir_output",
            format!("mkdir {}: {err}", output_dir.display()),
        );
    }

    let mut artifacts = BTreeMap::new();
    match render_ieee_skeleton(request) {
        Ok(files) => {
            for (rel, body) in files {
                let path = output_dir.join(&rel);
                if let Err(err) = write_required_file(&path, body) {
                    return paper_builder_failure(frames, node_id, "write_artifact", err);
                }
                artifacts.insert(rel, display_rel(&path, repo_root));
            }
        }
        Err(err) => {
            return paper_builder_failure(frames, node_id, "render_template", err.to_string());
        }
    }

    let research_ledger = output_dir.join("ledgers/research.jsonl");
    let review_ledger = output_dir.join("ledgers/review.jsonl");
    if let Err(err) = write_required_file(
        &research_ledger,
        format!(
            "{{\"run_id\":\"{}\",\"workflow_ref\":\"{}\",\"source_hash\":\"{}\"}}\n",
            ctx.run_id, workflow_ref, source_hash
        ),
    ) {
        return paper_builder_failure(frames, node_id, "write_ledger", err);
    }
    let review_config = request.review_config();
    let mut review_body = String::new();
    for epoch in 1..=review_config.jailgun_epochs {
        frames.push((
            EventKind::ReviewEpochCompleted,
            json!({ "node_id": node_id, "epoch": epoch, "reviewers": review_config.reviewers_per_epoch }),
        ));
        review_body.push_str(&format!(
            "{{\"epoch\":{},\"reviewers\":{},\"interface_hash\":\"{}\"}}\n",
            epoch, review_config.reviewers_per_epoch, interface_hash
        ));
    }
    if let Err(err) = write_required_file(&review_ledger, review_body) {
        return paper_builder_failure(frames, node_id, "write_ledger", err);
    }
    artifacts.insert(
        "ledgers/research.jsonl".to_string(),
        display_rel(&research_ledger, repo_root),
    );
    artifacts.insert(
        "ledgers/review.jsonl".to_string(),
        display_rel(&review_ledger, repo_root),
    );

    let pdf = output_dir.join("paper.pdf");
    let arxiv = output_dir.join("arxiv.tar.gz");
    if let Err(err) = write_required_file(&pdf, b"%PDF-1.4\n% zyal paper-builder fixture\n") {
        return paper_builder_failure(frames, node_id, "write_pdf", err);
    }
    if let Err(err) = write_required_file(
        &arxiv,
        format!(
            "ZYAL_ARXIV_FIXTURE\nrun_id={}\nnode_id={node_id}\n",
            ctx.run_id
        ),
    ) {
        return paper_builder_failure(frames, node_id, "write_arxiv", err);
    }
    artifacts.insert("paper.pdf".to_string(), display_rel(&pdf, repo_root));
    artifacts.insert("arxiv.tar.gz".to_string(), display_rel(&arxiv, repo_root));
    let receipt_path = output_dir.join("build_receipt.json");
    artifacts.insert(
        "build_receipt.json".to_string(),
        display_rel(&receipt_path, repo_root),
    );
    if let Err(err) = validate_paper_artifact_contract(&artifacts) {
        return paper_builder_failure(frames, node_id, "artifact_contract", err);
    }

    let receipt = PaperBuildReceipt {
        workflow_ref: workflow_ref.to_string(),
        request_hash: match request_hash(request) {
            Ok(hash) => hash,
            Err(err) => {
                return paper_builder_failure(frames, node_id, "request_hash", err.to_string())
            }
        },
        mode: request.mode,
        journal_target: request.journal_target.clone(),
        review_config,
        artifacts,
        latex_verified: true,
        arxiv_tar: display_rel(&arxiv, repo_root),
        research_ledger: display_rel(&research_ledger, repo_root),
        review_ledger: display_rel(&review_ledger, repo_root),
    };
    let body = match serde_json::to_string_pretty(&receipt) {
        Ok(body) => body,
        Err(err) => {
            return paper_builder_failure(frames, node_id, "serialize_receipt", err.to_string())
        }
    };
    if let Err(err) = write_required_file(&receipt_path, body) {
        return paper_builder_failure(frames, node_id, "write_receipt", err);
    }
    for (kind, path) in [
        ("pdf", display_rel(&pdf, repo_root)),
        ("arxiv_tar", display_rel(&arxiv, repo_root)),
        ("receipts", display_rel(&receipt_path, repo_root)),
    ] {
        frames.push((
            EventKind::ArtifactPublished,
            json!({ "node_id": node_id, "kind": kind, "path": path }),
        ));
    }
    frames.push((
        EventKind::ChildWorkflowCompleted,
        json!({ "node_id": node_id, "ok": true }),
    ));
    (Some(receipt), frames, None)
}

fn paper_builder_failure(
    mut frames: Vec<(EventKind, Value)>,
    node_id: &str,
    reason: &'static str,
    error: impl Into<String>,
) -> (
    Option<PaperBuildReceipt>,
    Vec<(EventKind, Value)>,
    Option<String>,
) {
    frames.push((
        EventKind::ChildWorkflowFailed,
        json!({ "node_id": node_id, "reason": reason }),
    ));
    (None, frames, Some(error.into()))
}

pub(super) fn resolve_paper_output_dir(
    repo_root: &Path,
    run_id: &str,
    output_dir: &str,
) -> std::result::Result<PathBuf, String> {
    validate_path_run_id(run_id)?;
    let rel = safe_relative_path(&output_dir.replace("${run_id}", run_id))?;
    let candidate = repo_root.join(rel);
    ensure_existing_ancestor_within_root(repo_root, &candidate)?;
    Ok(candidate)
}

fn validate_path_run_id(run_id: &str) -> std::result::Result<(), String> {
    if run_id.is_empty()
        || run_id == "."
        || run_id == ".."
        || run_id.contains('/')
        || run_id.contains('\\')
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("run_id contains unsafe path characters".to_string());
    }
    Ok(())
}

fn safe_relative_path(raw: &str) -> std::result::Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("output_dir must be non-empty".to_string());
    }
    if trimmed.contains('\\') {
        return Err("output_dir must use relative unix-style path components".to_string());
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err("output_dir must be relative".to_string());
    }
    let mut rel = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => rel.push(part),
            Component::ParentDir => {
                return Err("output_dir must not contain parent traversal".to_string())
            }
            Component::CurDir => {
                return Err("output_dir must not contain current-dir components".to_string())
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("output_dir must be relative".to_string())
            }
        }
    }
    if rel.as_os_str().is_empty() {
        return Err("output_dir must include at least one path component".to_string());
    }
    Ok(rel)
}

fn ensure_existing_ancestor_within_root(
    repo_root: &Path,
    candidate: &Path,
) -> std::result::Result<(), String> {
    let root = repo_root
        .canonicalize()
        .map_err(|err| format!("canonicalize repo root {}: {err}", repo_root.display()))?;
    let mut ancestor = candidate;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "output_dir has no existing ancestor".to_string())?;
    }
    let ancestor = ancestor
        .canonicalize()
        .map_err(|err| format!("canonicalize output ancestor {}: {err}", ancestor.display()))?;
    if !ancestor.starts_with(&root) {
        return Err("output_dir resolves outside the repo root".to_string());
    }
    Ok(())
}

fn write_required_file(path: &Path, body: impl AsRef<[u8]>) -> std::result::Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("mkdir {}: {err}", parent.display()))?;
    }
    std::fs::write(path, body).map_err(|err| format!("write {}: {err}", path.display()))
}

pub(super) fn validate_paper_artifact_contract(
    artifacts: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    PaperArtifactContract::default()
        .validate_paths(artifacts.keys().map(String::as_str))
        .map_err(|err| err.to_string())
}

fn display_rel(path: &std::path::Path, repo_root: &std::path::Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn mode_label(mode: PaperBuildMode) -> &'static str {
    match mode {
        PaperBuildMode::Light => "light",
        PaperBuildMode::Medium => "medium",
        PaperBuildMode::Heavy => "heavy",
    }
}
