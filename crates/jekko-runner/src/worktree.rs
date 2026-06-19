//! Git worktree manager. Wraps `git worktree add/remove` via `std::process::Command`
//! following the convention established in `crates/sandboxctl/src/backend/worktree.rs`.
//!
//! Layout: `<repo>/.zyal/worktrees/<run_id>/<worker_id>/` for each worker.
//! Branch name: `<branch_prefix>/<run_id>/<worker_id>/<finding_id>` so a quick
//! `git branch --list 'zyal/*'` shows every active worker at a glance.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone)]
pub struct WorktreeHandle {
    pub run_id: String,
    pub worker_id: String,
    pub finding_id: String,
    pub branch: String,
    pub path: PathBuf,
}

pub struct WorktreeManager {
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    branch_prefix: String,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf, run_id: &str, branch_prefix: Option<&str>) -> Result<Self> {
        let worktrees_root = repo_root.join(".zyal/worktrees").join(run_id);
        fs::create_dir_all(&worktrees_root)
            .with_context(|| format!("mkdir -p {}", worktrees_root.display()))?;
        Ok(Self {
            repo_root,
            worktrees_root,
            branch_prefix: branch_prefix.unwrap_or("zyal").to_string(),
        })
    }

    pub fn create(
        &self,
        worker_id: &str,
        finding_id: &str,
        base_branch: &str,
    ) -> Result<WorktreeHandle> {
        let path = self.worktrees_root.join(worker_id);
        if path.exists() {
            return Err(anyhow!("worktree already exists at {}", path.display()));
        }
        let branch = format!(
            "{}/{}/{}/{}",
            self.branch_prefix,
            self.worktrees_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("run"),
            worker_id,
            sanitize_branch_segment(finding_id),
        );
        let status = Command::new("git")
            .args(["worktree", "add", "-b", &branch])
            .arg(&path)
            .arg(base_branch)
            .current_dir(&self.repo_root)
            .status()
            .with_context(|| format!("git worktree add -> {}", path.display()))?;
        if !status.success() {
            return Err(anyhow!(
                "git worktree add returned {} for branch {}",
                status.code().unwrap_or(-1),
                branch
            ));
        }
        Ok(WorktreeHandle {
            run_id: self
                .worktrees_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("run")
                .to_string(),
            worker_id: worker_id.to_string(),
            finding_id: finding_id.to_string(),
            branch,
            path,
        })
    }

    pub fn remove(&self, handle: &WorktreeHandle) -> Result<()> {
        let status = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&handle.path)
            .current_dir(&self.repo_root)
            .status()
            .with_context(|| format!("git worktree remove {}", handle.path.display()))?;
        if !status.success() {
            // Recovery branch: if `git worktree remove` fails because the dir
            // was already deleted, manually clean the registry.
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&self.repo_root)
                .status();
        }
        // Delete the branch if it still points at the handle's name. Best-effort.
        let _ = Command::new("git")
            .args(["branch", "-D", &handle.branch])
            .current_dir(&self.repo_root)
            .status();
        Ok(())
    }

    /// Remove worktrees idle longer than `idle`. Returns the count pruned.
    pub fn gc(&self, idle: Duration) -> Result<usize> {
        let mut pruned = 0usize;
        let now = SystemTime::now();
        let entries = match fs::read_dir(&self.worktrees_root) {
            Ok(rd) => rd,
            Err(_) => return Ok(0),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = metadata.modified().unwrap_or(now);
            let age = match now.duration_since(mtime) {
                Ok(d) => d,
                Err(_) => std::time::Duration::ZERO,
            };
            if age > idle {
                let status = Command::new("git")
                    .args(["worktree", "remove", "--force"])
                    .arg(&path)
                    .current_dir(&self.repo_root)
                    .status();
                if status.map(|s| s.success()).unwrap_or(false) {
                    pruned += 1;
                } else {
                    // dir may not be a registered worktree any more; rm by hand
                    let _ = fs::remove_dir_all(&path);
                    pruned += 1;
                }
            }
        }
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.repo_root)
            .status();
        Ok(pruned)
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn worktrees_root(&self) -> &Path {
        &self.worktrees_root
    }

    pub fn branch_prefix(&self) -> &str {
        &self.branch_prefix
    }
}

fn sanitize_branch_segment(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_branch_segment() {
        assert_eq!(
            sanitize_branch_segment("HLT-001/dead marker"),
            "HLT-001-dead-marker"
        );
        assert_eq!(
            sanitize_branch_segment("cap:no-security-lane"),
            "cap-no-security-lane"
        );
    }
}
