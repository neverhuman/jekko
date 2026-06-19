use super::plan::default_memory_scope;
use super::{
    MasterTaskStatus, PhaseStatus, PortMasterPlan, PortMasterTask, PortStage, PortTargetRequest,
};

/// Build a generic starter plan without target-specific hard-coding.
pub fn draft_master_plan(target: PortTargetRequest) -> PortMasterPlan {
    let names = [
        (
            "discover",
            "Discover target behavior, docs, tests, and public contracts.",
        ),
        (
            "protocol_map",
            "Map wire/protocol semantics, error surfaces, and compatibility boundaries.",
        ),
        (
            "skeleton",
            "Create the replacement project skeleton and compatibility adapters.",
        ),
        (
            "case_harness",
            "Build target-switched golden cases and replay harnesses before implementation.",
        ),
        (
            "core_semantics",
            "Implement core command semantics behind approved parity cases.",
        ),
        (
            "persistence",
            "Close persistence, restart, and durability behavior with replay evidence.",
        ),
        (
            "concurrency",
            "Implement concurrency, pipelining, and ordering guarantees.",
        ),
        (
            "operations",
            "Add observability, configuration, failure-mode, and deployment controls.",
        ),
        (
            "integration",
            "Fuse phases and heal cross-phase regressions.",
        ),
        (
            "parity",
            "Close exhaustive correctness and performance parity gaps.",
        ),
    ];
    let stages: Vec<PortStage> = names
        .iter()
        .enumerate()
        .map(|(idx, (id, objective))| PortStage {
            id: format!("stage-{}", id),
            ordinal: idx + 1,
            name: id.to_string(),
            objective: (*objective).to_string(),
            status: if idx == 0 {
                PhaseStatus::Drafting
            } else {
                PhaseStatus::Planned
            },
            dependencies: if idx == 0 {
                Vec::new()
            } else {
                vec![format!("stage-{}", names[idx - 1].0)]
            },
            parallel_group: Some(format!("group-{:02}", idx + 1)),
            write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
            proof_lanes: vec!["just zyal-port-fast".to_string()],
            signoff_evidence: vec![
                "proof_receipt".to_string(),
                "replay_receipt".to_string(),
                "parity_receipt".to_string(),
            ],
        })
        .collect();
    let tasks = stages
        .iter()
        .enumerate()
        .map(|(idx, stage)| {
            let task_id = format!("task-{}", stage.name);
            let dependencies = if idx == 0 {
                Vec::new()
            } else {
                vec![format!("task-{}", stages[idx - 1].name)]
            };
            PortMasterTask {
                id: task_id,
                stage_id: stage.id.clone(),
                title: format!("{}: {}", target.replacement, stage.objective),
                task_kind: stage.name.clone(),
                risk_level: if matches!(stage.name.as_str(), "parity" | "operations") {
                    "high".to_string()
                } else {
                    "medium".to_string()
                },
                write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
                bounded_write_scope: true,
                dependencies,
                proof_lane: "just zyal-port-fast".to_string(),
                done_evidence: vec![
                    "tests_passed".to_string(),
                    "replay_receipt".to_string(),
                    "jankurai_gate_passed".to_string(),
                    "parity_receipt".to_string(),
                ],
                memory_scope: default_memory_scope(),
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
