use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

use crate::evidence::LoadedEvidence;
use crate::port::{
    MasterTaskStatus, PhaseStatus, PortMasterPlan, PortMasterTask, PortStage, PortTargetRequest,
};

use super::helpers::{evidence_topics, slug};

pub(crate) fn build_stage0_master_plan(
    target: PortTargetRequest,
    evidence: &[LoadedEvidence],
) -> PortMasterPlan {
    let topics = evidence_topics(evidence, 8);
    let topics = if topics.is_empty() {
        vec![
            "contract".to_string(),
            "behavior".to_string(),
            "parity".to_string(),
        ]
    } else {
        topics
    };
    let stages: Vec<PortStage> = topics
        .iter()
        .enumerate()
        .map(|(idx, topic)| PortStage {
            id: format!("stage-{:02}-{}", idx + 1, slug(topic)),
            ordinal: idx + 1,
            name: format!("{topic} evidence"),
            objective: format!(
                "Derive, implement, and verify target behavior for evidence topic `{topic}`."
            ),
            status: if idx == 0 {
                PhaseStatus::Drafting
            } else {
                PhaseStatus::Planned
            },
            dependencies: if idx == 0 {
                Vec::new()
            } else {
                vec![format!("stage-{:02}-{}", idx, slug(&topics[idx - 1]))]
            },
            parallel_group: Some(format!("stage0-{:02}", idx + 1)),
            write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
            proof_lanes: vec!["rtk just zyal-port-fast".to_string()],
            signoff_evidence: vec!["stage0_proof".to_string(), "parity_summary".to_string()],
        })
        .collect();
    let tasks = stages
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
                title: format!(
                    "{}: close target-derived behavior for {}",
                    target.replacement, stage.name
                ),
                task_kind: "correctness".to_string(),
                risk_level: "medium".to_string(),
                write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
                bounded_write_scope: true,
                dependencies,
                proof_lane: "rtk just zyal-port-fast".to_string(),
                done_evidence: vec![
                    "stage0_proof".to_string(),
                    "tests_passed".to_string(),
                    "replay_receipt".to_string(),
                ],
                memory_scope: "run".to_string(),
                generated_zone_boundary_checks: true,
                status: MasterTaskStatus::Queued,
            }
        })
        .collect();
    PortMasterPlan {
        target,
        stages,
        tasks,
    }
}

pub(crate) fn write_stage0_master_plan(
    repo: &Path,
    run_id: &str,
    plan: &PortMasterPlan,
    evidence: &[LoadedEvidence],
) -> Result<PathBuf> {
    let path = repo
        .join("target/zyal/reasoning")
        .join(run_id)
        .join("stage0-master-plan.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let payload = json!({
        "schema_version": "zyal.stage0_master_plan.v1",
        "run_id": run_id,
        "source": "runtime_evidence",
        "evidence": evidence.iter().map(LoadedEvidence::receipt).collect::<Vec<_>>(),
        "plan": plan,
    });
    fs::write(&path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}
