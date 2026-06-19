//! Safe worker-pool planning for port tasks.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::port::{MasterTaskStatus, PortMasterTask, MAX_PORT_WORKERS};

/// Worker-pool policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerPoolPolicy {
    /// Maximum workers allowed for this run.
    pub max_workers: usize,
    /// Path prefixes workers may not edit directly.
    pub forbidden_prefixes: Vec<String>,
}

impl Default for WorkerPoolPolicy {
    fn default() -> Self {
        Self {
            max_workers: MAX_PORT_WORKERS,
            forbidden_prefixes: vec![
                ".git/".to_string(),
                ".jankurai/".to_string(),
                ".zyal/".to_string(),
                "target/".to_string(),
            ],
        }
    }
}

impl WorkerPoolPolicy {
    /// Construct a policy and enforce the hard worker cap.
    pub fn new(max_workers: usize) -> Result<Self> {
        if max_workers == 0 || max_workers > MAX_PORT_WORKERS {
            return Err(anyhow!(
                "worker cap must be between 1 and {MAX_PORT_WORKERS}, got {max_workers}"
            ));
        }
        Ok(Self {
            max_workers,
            ..Self::default()
        })
    }
}

/// Planned worker assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerAssignment {
    /// Worker id.
    pub worker_id: String,
    /// Task id.
    pub task_id: String,
    /// Isolated worktree path.
    pub worktree_path: PathBuf,
    /// Scoped branch name.
    pub branch: String,
    /// Declared write scope.
    pub write_scope: Vec<String>,
    /// Proof lane.
    pub proof_lane: String,
}

/// Plan assignments for currently assignable tasks.
pub fn plan_assignments(
    run_id: &str,
    repo_root: impl Into<PathBuf>,
    tasks: &[PortMasterTask],
    policy: &WorkerPoolPolicy,
) -> Result<Vec<WorkerAssignment>> {
    let repo_root = repo_root.into();
    let mut assignments = Vec::new();
    let statuses = tasks
        .iter()
        .map(|task| (task.id.as_str(), task.status))
        .collect::<BTreeMap<_, _>>();
    for task in tasks
        .iter()
        .filter(|task| task.status.is_assignable())
        .filter(|task| dependencies_resolved(task, &statuses))
    {
        if assignments.len() >= policy.max_workers {
            break;
        }
        validate_write_scope(&task.write_scope, policy)?;
        let worker_id = format!("worker-{:02}", assignments.len() + 1);
        let branch = format!(
            "zyal/{run_id}/{worker_id}/{}",
            sanitize_branch_segment(&task.id)
        );
        assignments.push(WorkerAssignment {
            worktree_path: repo_root
                .join(".zyal/worktrees")
                .join(run_id)
                .join(&worker_id),
            worker_id,
            task_id: task.id.clone(),
            branch,
            write_scope: task.write_scope.clone(),
            proof_lane: task.proof_lane.clone(),
        });
    }
    Ok(assignments)
}

fn dependencies_resolved(
    task: &PortMasterTask,
    statuses: &BTreeMap<&str, MasterTaskStatus>,
) -> bool {
    task.dependencies.iter().all(|dep| {
        statuses
            .get(dep.as_str())
            .map(|status| matches!(status, MasterTaskStatus::Done | MasterTaskStatus::Merged))
            .unwrap_or(false)
    })
}

/// Validate a declared write scope against the policy.
pub fn validate_write_scope(paths: &[String], policy: &WorkerPoolPolicy) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!(
            "worker task must declare at least one write-scope path"
        ));
    }
    for raw in paths {
        let normalized = normalize_scope(raw);
        if policy.forbidden_prefixes.iter().any(|prefix| {
            normalized == prefix.trim_end_matches('/') || normalized.starts_with(prefix)
        }) {
            return Err(anyhow!("write scope {raw:?} is forbidden"));
        }
    }
    Ok(())
}

fn normalize_scope(raw: &str) -> String {
    raw.trim_start_matches("./").to_string()
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

    fn task(id: &str, status: MasterTaskStatus, scope: &[&str]) -> PortMasterTask {
        PortMasterTask {
            id: id.to_string(),
            stage_id: "stage".to_string(),
            title: id.to_string(),
            task_kind: "implementation".to_string(),
            risk_level: "medium".to_string(),
            write_scope: scope.iter().map(|s| (*s).to_string()).collect(),
            bounded_write_scope: true,
            dependencies: Vec::new(),
            proof_lane: "just fast".to_string(),
            done_evidence: vec!["tests".to_string()],
            memory_scope: "run".to_string(),
            generated_zone_boundary_checks: true,
            status,
        }
    }

    fn task_with_deps(
        id: &str,
        status: MasterTaskStatus,
        scope: &[&str],
        dependencies: &[&str],
    ) -> PortMasterTask {
        let mut task = task(id, status, scope);
        task.dependencies = dependencies.iter().map(|dep| (*dep).to_string()).collect();
        task
    }

    #[test]
    fn rejects_worker_count_above_ten() {
        assert!(WorkerPoolPolicy::new(11).is_err());
        assert!(WorkerPoolPolicy::new(10).is_ok());
    }

    #[test]
    fn plans_only_assignable_tasks() {
        let policy = WorkerPoolPolicy::new(2).unwrap();
        let assignments = plan_assignments(
            "run-1",
            "/tmp/repo",
            &[
                task("a", MasterTaskStatus::Queued, &["src/a.rs"]),
                task("b", MasterTaskStatus::Running, &["src/b.rs"]),
                task("c", MasterTaskStatus::ProofFailed, &["src/c.rs"]),
            ],
            &policy,
        )
        .unwrap();
        assert_eq!(
            assignments
                .iter()
                .map(|a| a.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
        assert!(assignments[0].branch.starts_with("zyal/run-1/worker-01"));
    }

    #[test]
    fn rejects_forbidden_scope() {
        let policy = WorkerPoolPolicy::default();
        let err =
            validate_write_scope(&["target/zyal/report.json".to_string()], &policy).unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[test]
    fn blocks_assignment_until_dependencies_are_resolved() {
        let policy = WorkerPoolPolicy::new(3).unwrap();
        let assignments = plan_assignments(
            "run-1",
            "/tmp/repo",
            &[
                task("a", MasterTaskStatus::Queued, &["src/a.rs"]),
                task_with_deps("b", MasterTaskStatus::Queued, &["src/b.rs"], &["a"]),
                task_with_deps("c", MasterTaskStatus::Queued, &["src/c.rs"], &["missing"]),
            ],
            &policy,
        )
        .unwrap();
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );

        let assignments = plan_assignments(
            "run-1",
            "/tmp/repo",
            &[
                task("a", MasterTaskStatus::Done, &["src/a.rs"]),
                task_with_deps("b", MasterTaskStatus::Queued, &["src/b.rs"], &["a"]),
            ],
            &policy,
        )
        .unwrap();
        assert_eq!(assignments[0].task_id, "b");
    }
}
