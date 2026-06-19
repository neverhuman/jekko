use super::*;

#[test]
fn worker_cap_is_clamped_to_ten() {
    let req = PortTargetRequest {
        target: "Reference".into(),
        replacement: "Candidate".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port it".into(),
        worker_cap: 20,
    };
    assert_eq!(req.effective_worker_cap(), MAX_PORT_WORKERS);
}

#[test]
fn starter_plan_is_generic_and_ordered() {
    let req = PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    };
    let plan = draft_master_plan(req);
    assert_eq!(plan.stages.first().unwrap().name, "discover");
    assert_eq!(plan.stages.last().unwrap().name, "parity");
    assert!((9..=12).contains(&plan.stages.len()));
    assert_eq!(plan.tasks.len(), plan.stages.len());
    assert!(plan
        .tasks
        .iter()
        .all(|t| t.status == MasterTaskStatus::Queued));
    validate_master_plan_contract(&plan).unwrap();
}

#[test]
fn master_plan_rejects_invalid_dependencies() {
    let req = PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    };
    let mut plan = draft_master_plan(req);
    plan.tasks[0].dependencies.push("missing-task".into());
    assert!(validate_master_plan_contract(&plan)
        .unwrap_err()
        .to_string()
        .contains("invalid dependency"));
}

#[test]
fn master_plan_rejects_unbounded_write_scope() {
    let req = PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    };
    let mut plan = draft_master_plan(req);
    plan.tasks[0].write_scope = vec!["**/*".into()];
    assert!(validate_master_plan_contract(&plan)
        .unwrap_err()
        .to_string()
        .contains("unbounded write scope"));
}

#[test]
fn master_plan_rejects_duplicate_ids_and_cycles() {
    let req = PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    };
    let mut duplicate = draft_master_plan(req.clone());
    duplicate.tasks[1].id = duplicate.tasks[0].id.clone();
    assert!(validate_master_plan_contract(&duplicate)
        .unwrap_err()
        .to_string()
        .contains("duplicate task ids"));

    let mut cycle = draft_master_plan(req);
    let last_id = cycle.tasks.last().unwrap().id.clone();
    cycle.tasks[0].dependencies.push(last_id);
    assert!(validate_master_plan_contract(&cycle)
        .unwrap_err()
        .to_string()
        .contains("dependency cycle"));
}

#[test]
fn master_plan_requires_proof_evidence() {
    let req = PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    };
    let mut plan = draft_master_plan(req);
    plan.tasks[0].done_evidence.clear();
    assert!(validate_master_plan_contract(&plan)
        .unwrap_err()
        .to_string()
        .contains("done evidence"));
}
