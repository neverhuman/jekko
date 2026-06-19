use anyhow::{anyhow, Result};

use crate::port::{
    validate_master_plan_contract, MasterTaskStatus, PhaseStatus, PortMasterPlan, PortMasterTask,
    PortStage, PortTargetRequest,
};

use super::helpers::{bool_field, slug, string_array_field, string_field};

pub(crate) fn parse_model_master_plan(
    target: PortTargetRequest,
    value: &serde_json::Value,
) -> Result<PortMasterPlan> {
    let plan_value = value.get("plan").unwrap_or(value);
    if let Ok(mut plan) = serde_json::from_value::<PortMasterPlan>(plan_value.clone()) {
        plan.target = target;
        validate_master_plan(&plan)?;
        return Ok(plan);
    }
    let stages_value = plan_value
        .get("stages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("missing stages array"))?;
    let mut stages = Vec::new();
    for (idx, stage) in stages_value.iter().enumerate() {
        let name =
            string_field(stage, &["name", "title"]).unwrap_or_else(|| format!("stage {}", idx + 1));
        let id = string_field(stage, &["id"])
            .unwrap_or_else(|| format!("stage-{:02}-{}", idx + 1, slug(&name)));
        let objective = string_field(stage, &["objective", "summary", "description"])
            .unwrap_or_else(|| format!("Complete target-derived work for {name}."));
        let dependencies: Vec<String> = string_array_field(stage, "dependencies")
            .into_iter()
            .flatten()
            .collect();
        stages.push(PortStage {
            id,
            ordinal: idx + 1,
            name,
            objective,
            status: if idx == 0 {
                PhaseStatus::Drafting
            } else {
                PhaseStatus::Planned
            },
            dependencies,
            parallel_group: string_field(stage, &["parallel_group"]),
            write_scope: string_array_field(stage, "write_scope")
                .unwrap_or_else(|| vec!["src/**".to_string(), "tests/**".to_string()]),
            proof_lanes: string_array_field(stage, "proof_lanes")
                .unwrap_or_else(|| vec!["rtk just zyal-port-fast".to_string()]),
            signoff_evidence: string_array_field(stage, "signoff_evidence")
                .unwrap_or_else(|| vec!["proof_receipt".to_string()]),
        });
    }
    let mut tasks: Vec<PortMasterTask> = Vec::new();
    if let Some(task_values) = plan_value
        .get("tasks")
        .and_then(serde_json::Value::as_array)
    {
        for (idx, task) in task_values.iter().enumerate() {
            let title = string_field(task, &["title", "name"])
                .unwrap_or_else(|| format!("task {}", idx + 1));
            let default_stage = stages
                .get(idx.min(stages.len().saturating_sub(1)))
                .map(|stage| stage.id.clone())
                .unwrap_or_else(|| "stage-01".to_string());
            let stage_id = string_field(task, &["stage_id", "phase_id"]).unwrap_or(default_stage);
            let id = string_field(task, &["id"])
                .unwrap_or_else(|| format!("task-{:02}-{}", idx + 1, slug(&title)));
            let task_dependencies = string_array_field(task, "dependencies").unwrap_or_else(|| {
                if idx == 0 {
                    Vec::new()
                } else {
                    vec![tasks[idx - 1].id.clone()]
                }
            });
            tasks.push(PortMasterTask {
                id,
                stage_id,
                title,
                task_kind: string_field(task, &["task_kind", "kind"])
                    .unwrap_or_else(|| "implementation".to_string()),
                risk_level: string_field(task, &["risk_level", "risk"])
                    .unwrap_or_else(|| "medium".to_string()),
                write_scope: string_array_field(task, "write_scope")
                    .unwrap_or_else(|| vec!["src/**".to_string(), "tests/**".to_string()]),
                bounded_write_scope: bool_field(task, "bounded_write_scope").unwrap_or(true),
                dependencies: task_dependencies,
                proof_lane: string_field(task, &["proof_lane", "proof"])
                    .unwrap_or_else(|| "rtk just zyal-port-fast".to_string()),
                done_evidence: string_array_field(task, "done_evidence").unwrap_or_else(|| {
                    vec!["tests_passed".to_string(), "replay_receipt".to_string()]
                }),
                memory_scope: string_field(task, &["memory_scope"])
                    .unwrap_or_else(|| "run".to_string()),
                generated_zone_boundary_checks: bool_field(task, "generated_zone_boundary_checks")
                    .unwrap_or(true),
                status: MasterTaskStatus::Queued,
            });
        }
    }
    if tasks.is_empty() {
        tasks = stages
            .iter()
            .enumerate()
            .map(|(idx, stage)| {
                let id = format!("task-{}", stage.id.trim_start_matches("stage-"));
                let dependencies = if idx == 0 {
                    Vec::new()
                } else {
                    vec![format!(
                        "task-{}",
                        stages[idx - 1].id.trim_start_matches("stage-")
                    )]
                };
                PortMasterTask {
                    id,
                    stage_id: stage.id.clone(),
                    title: format!("Implement and verify {}", stage.name),
                    task_kind: "implementation".to_string(),
                    risk_level: "medium".to_string(),
                    write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
                    bounded_write_scope: true,
                    dependencies,
                    proof_lane: "rtk just zyal-port-fast".to_string(),
                    done_evidence: vec!["tests_passed".to_string(), "replay_receipt".to_string()],
                    memory_scope: "run".to_string(),
                    generated_zone_boundary_checks: true,
                    status: MasterTaskStatus::Queued,
                }
            })
            .collect();
    }
    let plan = PortMasterPlan {
        target,
        stages,
        tasks,
    };
    validate_master_plan(&plan)?;
    Ok(plan)
}

fn validate_master_plan(plan: &PortMasterPlan) -> Result<()> {
    if plan.stages.is_empty() {
        anyhow::bail!("master plan has no stages");
    }
    if plan.tasks.is_empty() {
        anyhow::bail!("master plan has no tasks");
    }
    let stage_ids = plan
        .stages
        .iter()
        .map(|stage| stage.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for stage in &plan.stages {
        if stage.id.trim().is_empty() || stage.name.trim().is_empty() {
            anyhow::bail!("stage id and name are required");
        }
    }
    for task in &plan.tasks {
        if task.id.trim().is_empty() || task.title.trim().is_empty() {
            anyhow::bail!("task id and title are required");
        }
        if !stage_ids.contains(task.stage_id.as_str()) {
            anyhow::bail!(
                "task {} references unknown stage {}",
                task.id,
                task.stage_id
            );
        }
    }
    validate_master_plan_contract(plan)
}
