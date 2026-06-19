//! End-to-end integration: real git repo, real `git worktree add`, real
//! commit message rendering, real rollback. Catches wiring drift between the
//! unit tests that exercise each module in isolation.

use std::fs;
use std::path::Path;
use std::process::Command;

use jekko_runner::commit::{assert_refspec_scope, build_message, CommitPlan};
use jekko_runner::rollback::{rollback_worktree, RollbackPlan};
use jekko_runner::worktree::WorktreeManager;
use tempfile::tempdir;

fn init_repo(repo: &Path) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .status()
        .unwrap();
    fs::write(repo.join("README.md"), "seed").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "seed"])
        .current_dir(repo)
        .status()
        .unwrap();
}

#[test]
fn create_worker_branch_then_commit_then_rollback() {
    let dir = tempdir().unwrap();
    let repo = dir.path().to_path_buf();
    init_repo(&repo);

    let manager = WorktreeManager::new(repo.clone(), "run-int-1", Some("zyal")).unwrap();
    let handle = manager.create("w-01", "HLT-001", "HEAD").unwrap();
    assert!(handle.path.exists(), "worktree dir not created");
    assert!(
        handle.branch.starts_with("zyal/"),
        "branch outside zyal/* namespace"
    );

    // Worker writes one declared file inside the worktree.
    let declared_rel = "src/a.rs";
    let declared_abs = handle.path.join(declared_rel);
    if let Some(parent) = declared_abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&declared_abs, "fn main() {}").unwrap();

    // Build a commit message and confirm shape + refspec scope before any push.
    let plan = CommitPlan {
        worktree: handle.path.clone(),
        changed_paths: vec![declared_rel.into()],
        rule_id: "HLT-001".to_string(),
        finding_id: "fp-1".to_string(),
        zone: "demo".to_string(),
        worker_id: "w-01".to_string(),
        run_id: "run-int-1".to_string(),
        standard_version: "0.8.0".to_string(),
        short_summary: "add main".to_string(),
        branch: handle.branch.clone(),
        integration_branch: "zyal/run-int-1/integration".to_string(),
    };
    let message = build_message(&plan);
    assert!(message.subject.chars().count() <= 50);
    assert!(message.subject.starts_with("fix(demo): HLT-001"));
    assert!(message.body.contains("Run: run-int-1"));
    assert_refspec_scope(&plan.branch).unwrap();

    // Simulate a failed proof lane: rollback should remove the declared
    // change and leave README.md (outside the declared scope) untouched.
    let shared_before = fs::read_to_string(handle.path.join("README.md")).unwrap();
    rollback_worktree(&RollbackPlan {
        worktree: handle.path.clone(),
        declared_paths: vec![declared_rel.into()],
    })
    .unwrap();
    let shared_after = fs::read_to_string(handle.path.join("README.md")).unwrap();
    assert_eq!(
        shared_before, shared_after,
        "shared file must survive scoped clean"
    );
    assert!(
        !handle.path.join(declared_rel).exists(),
        "declared file should be cleaned"
    );

    // GC + removal returns the worktree slot for reuse.
    manager.remove(&handle).unwrap();
    assert!(!handle.path.exists(), "worktree dir should be removed");
}
