//! Rollback path for a worker whose proof lanes failed or whose rebase
//! conflicted. Two strict rules:
//!   1. `git reset --hard HEAD` brings the worktree back to its last green.
//!   2. `git clean -fdx -- <declared paths>` is scoped to the paths the worker
//!      claimed. Untouched files outside that scope must survive — accidental
//!      clobbering of shared state would defeat the whole pool design.
//!
//! After three failed attempts the finding is quarantined to
//! `agent/zyal/quarantine.jsonl` and not retried until human review.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct RollbackPlan {
    pub worktree: PathBuf,
    /// Paths the worker had declared. The clean step is restricted to these.
    pub declared_paths: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub run_id: String,
    pub worker_id: String,
    pub finding_id: String,
    pub rule_id: String,
    pub attempts: u32,
    pub reason: String,
    pub ts_epoch_secs: u64,
}

pub fn rollback_worktree(plan: &RollbackPlan) -> Result<()> {
    if !plan.worktree.exists() {
        return Err(anyhow!("worktree missing: {}", plan.worktree.display()));
    }
    let reset = Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .current_dir(&plan.worktree)
        .status()
        .with_context(|| format!("git reset --hard HEAD in {}", plan.worktree.display()))?;
    if !reset.success() {
        return Err(anyhow!(
            "git reset --hard HEAD returned {}",
            reset.code().unwrap_or(-1)
        ));
    }
    if plan.declared_paths.is_empty() {
        // Nothing else to clean. We don't run `git clean` globally — that's
        // the whole point of the scoped contract.
        return Ok(());
    }
    let mut cmd = Command::new("git");
    cmd.args(["clean", "-fdx", "--"]);
    for path in &plan.declared_paths {
        cmd.arg(path);
    }
    cmd.current_dir(&plan.worktree);
    let status = cmd
        .status()
        .with_context(|| format!("git clean in {}", plan.worktree.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "git clean returned {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Append a quarantine record for a finding that has failed `MAX_ATTEMPTS`
/// times. Idempotent — duplicate records are allowed because callers want a
/// full audit trail of every quarantine event.
pub fn append_quarantine(repo_root: &Path, record: &QuarantineRecord) -> Result<()> {
    let path = repo_root.join("agent/zyal/quarantine.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let line = serde_json::to_string(record).context("serialize quarantine record")?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn should_quarantine(attempts: u32) -> bool {
    attempts >= MAX_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_git(repo: &Path) {
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
        fs::write(repo.join("seed.txt"), "seed").unwrap();
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
    fn reset_reverts_modified_tracked_file() {
        let dir = tempdir().unwrap();
        init_git(dir.path());
        // Modify the tracked file
        fs::write(dir.path().join("seed.txt"), "mutation").unwrap();
        rollback_worktree(&RollbackPlan {
            worktree: dir.path().to_path_buf(),
            declared_paths: vec![PathBuf::from("seed.txt")],
        })
        .unwrap();
        let after = fs::read_to_string(dir.path().join("seed.txt")).unwrap();
        assert_eq!(after, "seed");
    }

    #[test]
    fn scoped_clean_removes_only_declared_untracked_files() {
        let dir = tempdir().unwrap();
        init_git(dir.path());
        fs::write(dir.path().join("declared.txt"), "junk").unwrap();
        fs::write(dir.path().join("shared.txt"), "valuable").unwrap();
        rollback_worktree(&RollbackPlan {
            worktree: dir.path().to_path_buf(),
            declared_paths: vec![PathBuf::from("declared.txt")],
        })
        .unwrap();
        assert!(
            !dir.path().join("declared.txt").exists(),
            "declared file should be cleaned"
        );
        assert!(
            dir.path().join("shared.txt").exists(),
            "shared file must survive scoped clean"
        );
    }

    #[test]
    fn rollback_with_no_declared_paths_resets_but_skips_clean() {
        let dir = tempdir().unwrap();
        init_git(dir.path());
        fs::write(dir.path().join("seed.txt"), "mutation").unwrap();
        fs::write(dir.path().join("untracked.txt"), "leave me alone").unwrap();
        rollback_worktree(&RollbackPlan {
            worktree: dir.path().to_path_buf(),
            declared_paths: vec![],
        })
        .unwrap();
        let seed_after = fs::read_to_string(dir.path().join("seed.txt")).unwrap();
        assert_eq!(seed_after, "seed");
        assert!(
            dir.path().join("untracked.txt").exists(),
            "no declared paths -> no clean"
        );
    }

    #[test]
    fn quarantine_threshold_triggers_at_max_attempts() {
        assert!(!should_quarantine(0));
        assert!(!should_quarantine(MAX_ATTEMPTS - 1));
        assert!(should_quarantine(MAX_ATTEMPTS));
        assert!(should_quarantine(MAX_ATTEMPTS + 1));
    }

    #[test]
    fn quarantine_append_creates_file_and_writes_jsonl() {
        let dir = tempdir().unwrap();
        let record = QuarantineRecord {
            run_id: "run-1".into(),
            worker_id: "w-02".into(),
            finding_id: "fp-x".into(),
            rule_id: "HLT-007".into(),
            attempts: 3,
            reason: "rebase conflict 3x".into(),
            ts_epoch_secs: 1_700_000_000,
        };
        append_quarantine(dir.path(), &record).unwrap();
        append_quarantine(dir.path(), &record).unwrap();
        let path = dir.path().join("agent/zyal/quarantine.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("HLT-007"));
    }
}
