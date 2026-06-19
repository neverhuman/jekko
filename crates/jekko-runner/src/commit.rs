//! Commit-on-green protocol. Three rules:
//!   1. Stage only paths the worker declared in `claims.changed_paths`. Never
//!      `git add -A` — defends against secret leakage and untracked junk.
//!   2. Subject is caveman-commit-style and capped at 50 chars. Trailers carry
//!      finding id / worker / run / standard version.
//!   3. Push refspec is *always* scoped to `zyal/*`. The integration branch
//!      is rebased onto via `--force-with-lease`. `main` is never touched.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

pub const SUBJECT_MAX_LEN: usize = 50;

#[derive(Debug, Clone)]
pub struct CommitPlan {
    pub worktree: PathBuf,
    /// Files the worker actually changed. Anything outside this list is left
    /// untouched on disk and never staged.
    pub changed_paths: Vec<PathBuf>,
    pub rule_id: String,
    pub finding_id: String,
    pub zone: String,
    pub worker_id: String,
    pub run_id: String,
    pub standard_version: String,
    pub short_summary: String,
    pub branch: String,
    pub integration_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMessage {
    pub subject: String,
    pub body: String,
}

impl CommitMessage {
    pub fn render(&self) -> String {
        if self.body.is_empty() {
            self.subject.clone()
        } else {
            format!("{}\n\n{}", self.subject, self.body)
        }
    }
}

pub fn build_message(plan: &CommitPlan) -> CommitMessage {
    let subject = build_subject(plan);
    let body = build_body(plan);
    CommitMessage { subject, body }
}

fn build_subject(plan: &CommitPlan) -> String {
    let zone = if plan.zone.is_empty() {
        "jankurai"
    } else {
        plan.zone.as_str()
    };
    let core = format!("fix({}): {} {}", zone, plan.rule_id, plan.short_summary);
    truncate_with_ellipsis(&core, SUBJECT_MAX_LEN)
}

fn build_body(plan: &CommitPlan) -> String {
    let mut lines = Vec::new();
    if !plan.finding_id.is_empty() {
        lines.push(format!("Finding-ID: {}", plan.finding_id));
    }
    if !plan.worker_id.is_empty() {
        lines.push(format!("Worker: {}", plan.worker_id));
    }
    if !plan.run_id.is_empty() {
        lines.push(format!("Run: {}", plan.run_id));
    }
    if !plan.standard_version.is_empty() {
        lines.push(format!("Standard-Version: {}", plan.standard_version));
    }
    lines.push("Co-Authored-By: jekko-runner <runner@zyal.local>".to_string());
    lines.join("\n")
}

fn truncate_with_ellipsis(raw: &str, max: usize) -> String {
    if raw.chars().count() <= max {
        return raw.to_string();
    }
    let mut out: String = raw.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Validates the push refspec scope. Returns `Err` if anything outside
/// `zyal/*` is being pushed, so the caller cannot accidentally write to
/// `main` or another protected branch.
pub fn assert_refspec_scope(refspec: &str) -> Result<()> {
    // Accept forms: `branch`, `+branch:branch`, `branch:branch`, all prefixed
    // by `zyal/`. Reject anything that isn't.
    let pushed = refspec.split(':').next().unwrap_or("");
    let pushed = pushed.trim_start_matches('+');
    if pushed == "zyal" || pushed.starts_with("zyal/") {
        Ok(())
    } else {
        Err(anyhow!(
            "refusing to push refspec {refspec:?}: outside zyal/* namespace"
        ))
    }
}

/// Stages declared paths only. Caller asserts the worktree path is the
/// worktree root (not an arbitrary subdirectory) before calling.
pub fn stage_declared_paths(plan: &CommitPlan) -> Result<()> {
    if plan.changed_paths.is_empty() {
        return Err(anyhow!("nothing to stage: changed_paths empty"));
    }
    let mut cmd = Command::new("git");
    cmd.arg("add").arg("--");
    for path in &plan.changed_paths {
        cmd.arg(path);
    }
    cmd.current_dir(&plan.worktree);
    let status = cmd
        .status()
        .with_context(|| format!("git add in {}", plan.worktree.display()))?;
    if !status.success() {
        return Err(anyhow!("git add returned {}", status.code().unwrap_or(-1)));
    }
    Ok(())
}

pub fn run_commit(plan: &CommitPlan, message: &CommitMessage) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("commit").arg("-m").arg(&message.subject);
    if !message.body.is_empty() {
        cmd.arg("-m").arg(&message.body);
    }
    cmd.current_dir(&plan.worktree);
    let status = cmd
        .status()
        .with_context(|| format!("git commit in {}", plan.worktree.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "git commit returned {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Rebase the worker branch onto the integration branch. On conflict the
/// caller should rollback and re-queue.
pub fn rebase_onto_integration(plan: &CommitPlan) -> Result<()> {
    let fetch = Command::new("git")
        .args(["fetch", "origin", "--quiet"])
        .current_dir(&plan.worktree)
        .status();
    // fetch is best-effort; we may be offline in tests
    let _ = fetch;

    let status = Command::new("git")
        .arg("rebase")
        .arg(&plan.integration_branch)
        .current_dir(&plan.worktree)
        .status()
        .with_context(|| {
            format!(
                "git rebase {} in {}",
                plan.integration_branch,
                plan.worktree.display()
            )
        })?;
    if !status.success() {
        // Abort the rebase so the worktree is back to a sane state.
        let _ = Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(&plan.worktree)
            .status();
        return Err(anyhow!(
            "git rebase {} returned {}",
            plan.integration_branch,
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Push the worker branch with `--force-with-lease`. Refuses to push any
/// refspec outside `zyal/*`.
pub fn push_zyal_branch(plan: &CommitPlan, remote: &str) -> Result<()> {
    assert_refspec_scope(&plan.branch)?;
    let status = Command::new("git")
        .args(["push", "--force-with-lease", remote])
        .arg(&plan.branch)
        .current_dir(&plan.worktree)
        .status()
        .with_context(|| format!("git push {} {}", remote, plan.branch))?;
    if !status.success() {
        return Err(anyhow!("git push returned {}", status.code().unwrap_or(-1)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> CommitPlan {
        CommitPlan {
            worktree: PathBuf::from("/tmp/wt"),
            changed_paths: vec![PathBuf::from("src/x.rs")],
            rule_id: "HLT-001".to_string(),
            finding_id: "fp-abc".to_string(),
            zone: "jekko".to_string(),
            worker_id: "w-01".to_string(),
            run_id: "20260512T1042-3f9a02".to_string(),
            standard_version: "0.8.0".to_string(),
            short_summary: "prune dead marker".to_string(),
            branch: "zyal/run/w-01/HLT-001".to_string(),
            integration_branch: "zyal/run/integration".to_string(),
        }
    }

    #[test]
    fn subject_obeys_50_char_cap() {
        let plan = CommitPlan {
            short_summary: "very long summary that exceeds the fifty character ceiling".to_string(),
            ..sample_plan()
        };
        let message = build_message(&plan);
        assert!(
            message.subject.chars().count() <= SUBJECT_MAX_LEN,
            "subject = {:?}",
            message.subject
        );
    }

    #[test]
    fn subject_includes_zone_and_rule() {
        let message = build_message(&sample_plan());
        assert!(
            message.subject.starts_with("fix(jekko): HLT-001"),
            "subject = {:?}",
            message.subject
        );
    }

    #[test]
    fn body_includes_required_trailers() {
        let message = build_message(&sample_plan());
        assert!(message.body.contains("Finding-ID: fp-abc"));
        assert!(message.body.contains("Worker: w-01"));
        assert!(message.body.contains("Run: 20260512T1042-3f9a02"));
        assert!(message.body.contains("Standard-Version: 0.8.0"));
        assert!(message.body.contains("Co-Authored-By: jekko-runner"));
    }

    #[test]
    fn refspec_scope_accepts_zyal_branches() {
        assert_refspec_scope("zyal/run/w-01/HLT-001").unwrap();
        assert_refspec_scope("+zyal/run/integration:zyal/run/integration").unwrap();
        assert_refspec_scope("zyal").unwrap();
    }

    #[test]
    fn refspec_scope_rejects_main_and_other_branches() {
        assert!(assert_refspec_scope("main").is_err());
        assert!(assert_refspec_scope("refs/heads/main").is_err());
        assert!(assert_refspec_scope("feature/x").is_err());
        assert!(assert_refspec_scope("+main:main").is_err());
    }

    #[test]
    fn empty_zone_falls_back_to_jankurai() {
        let plan = CommitPlan {
            zone: String::new(),
            ..sample_plan()
        };
        let message = build_message(&plan);
        assert!(
            message.subject.starts_with("fix(jankurai):"),
            "subject = {:?}",
            message.subject
        );
    }

    #[test]
    fn render_combines_subject_and_body() {
        let message = build_message(&sample_plan());
        let rendered = message.render();
        assert!(rendered.starts_with("fix(jekko): HLT-001"));
        assert!(rendered.contains("\n\nFinding-ID:"));
    }
}
