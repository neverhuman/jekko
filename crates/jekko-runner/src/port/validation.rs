use std::collections::{BTreeMap, BTreeSet};

use super::PortMasterPlan;

/// Validate dependency and bounded-write-scope invariants.
pub fn validate_master_plan_contract(plan: &PortMasterPlan) -> anyhow::Result<()> {
    let stage_ids = plan
        .stages
        .iter()
        .map(|stage| stage.id.as_str())
        .collect::<BTreeSet<_>>();
    if stage_ids.len() != plan.stages.len() {
        anyhow::bail!("master plan has duplicate stage ids");
    }
    let task_ids = plan
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    if task_ids.len() != plan.tasks.len() {
        anyhow::bail!("master plan has duplicate task ids");
    }
    for stage in &plan.stages {
        if stage.proof_lanes.is_empty() {
            anyhow::bail!("stage {} has no proof lanes", stage.id);
        }
        if stage.signoff_evidence.is_empty() {
            anyhow::bail!("stage {} has no signoff evidence", stage.id);
        }
        for dep in &stage.dependencies {
            if !stage_ids.contains(dep.as_str()) {
                anyhow::bail!("stage {} has invalid dependency {}", stage.id, dep);
            }
        }
        if stage
            .write_scope
            .iter()
            .any(|scope| is_unbounded_scope(scope))
        {
            anyhow::bail!("stage {} has unbounded write scope", stage.id);
        }
    }
    for task in &plan.tasks {
        if !stage_ids.contains(task.stage_id.as_str()) {
            anyhow::bail!(
                "task {} references unknown stage {}",
                task.id,
                task.stage_id
            );
        }
        if task.write_scope.is_empty()
            || !task.bounded_write_scope
            || task
                .write_scope
                .iter()
                .any(|scope| is_unbounded_scope(scope))
        {
            anyhow::bail!("task {} has unbounded write scope", task.id);
        }
        if task.proof_lane.trim().is_empty() {
            anyhow::bail!("task {} has no proof lane", task.id);
        }
        if task.done_evidence.is_empty() {
            anyhow::bail!("task {} has no done evidence", task.id);
        }
        for dep in &task.dependencies {
            if !task_ids.contains(dep.as_str()) && !stage_ids.contains(dep.as_str()) {
                anyhow::bail!("task {} has invalid dependency {}", task.id, dep);
            }
        }
    }
    ensure_acyclic(
        plan.stages.iter().map(|stage| {
            (
                stage.id.as_str(),
                stage
                    .dependencies
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        }),
        "stage",
    )?;
    ensure_acyclic(
        plan.tasks.iter().map(|task| {
            (
                task.id.as_str(),
                task.dependencies
                    .iter()
                    .filter(|dep| task_ids.contains(dep.as_str()))
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        }),
        "task",
    )?;
    Ok(())
}

fn ensure_acyclic<'a>(
    nodes: impl IntoIterator<Item = (&'a str, Vec<&'a str>)>,
    label: &str,
) -> anyhow::Result<()> {
    let graph = nodes.into_iter().collect::<BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys().copied() {
        visit_node(node, &graph, &mut visiting, &mut visited, label)?;
    }
    Ok(())
}

fn visit_node<'a>(
    node: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
    label: &str,
) -> anyhow::Result<()> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node) {
        anyhow::bail!("{label} dependency cycle includes {node}");
    }
    for dep in graph.get(node).into_iter().flatten().copied() {
        if graph.contains_key(dep) {
            visit_node(dep, graph, visiting, visited, label)?;
        }
    }
    visiting.remove(node);
    visited.insert(node);
    Ok(())
}

fn is_unbounded_scope(scope: &str) -> bool {
    matches!(scope.trim(), "" | "." | "./" | "**" | "**/*" | "/" | "*")
}
