use tempfile::tempdir;

use super::*;
use crate::hashing::sha256_hex;
use crate::model_policy::ModelPolicy;
use crate::port::PortTargetRequest;

fn super_target() -> PortTargetRequest {
    PortTargetRequest {
        target: "Reference".into(),
        replacement: "Candidate".into(),
        target_repo: None,
        replacement_repo: None,
        request: "replace Reference with Candidate".into(),
        worker_cap: 10,
    }
}

#[test]
fn superreasoning_packet_hash_is_stable() {
    let dir = tempdir().unwrap();
    let packet = SuperReasoningPacket::hero_judge(
        "run-1",
        "objective",
        &SuperReasoningConfig::default(),
        dir.path(),
        "source-hash".to_string(),
        SuperReasoningBudgetContract {
            effective_generations: 1,
            model_call_budget: 8,
            search_query_budget: 1,
            search_page_budget: 2,
            max_parallel: 2,
            max_workers: 2,
        },
        ModelPolicy::default(),
    );
    assert_eq!(packet.stable_hash, packet.compute_hash());
    assert_eq!(packet.policy_hash, packet.compute_policy_hash());
    assert_eq!(packet.replay_receipt.status, "pending");
    packet.validate().unwrap();
}

#[test]
fn superreasoning_packet_rejects_raw_reasoning() {
    let dir = tempdir().unwrap();
    let mut packet = SuperReasoningPacket::hero_judge(
        "run-1",
        "objective",
        &SuperReasoningConfig::default(),
        dir.path(),
        "source-hash".to_string(),
        SuperReasoningBudgetContract {
            effective_generations: 1,
            model_call_budget: 8,
            search_query_budget: 1,
            search_page_budget: 2,
            max_parallel: 2,
            max_workers: 2,
        },
        ModelPolicy::default(),
    );
    packet.privacy_contract.store_raw_reasoning = true;
    packet.policy_hash = packet.compute_policy_hash();
    packet.stable_hash = packet.compute_hash();
    assert!(packet
        .validate()
        .unwrap_err()
        .to_string()
        .contains("raw reasoning"));
}

#[test]
fn superreasoning_packet_requires_negative_memory() {
    let dir = tempdir().unwrap();
    let mut packet = SuperReasoningPacket::hero_judge(
        "run-1",
        "objective",
        &SuperReasoningConfig::default(),
        dir.path(),
        "source-hash".to_string(),
        SuperReasoningBudgetContract {
            effective_generations: 1,
            model_call_budget: 8,
            search_query_budget: 1,
            search_page_budget: 2,
            max_parallel: 2,
            max_workers: 2,
        },
        ModelPolicy::default(),
    );
    packet.artifact_contract.negative_memory.clear();
    packet.policy_hash = packet.compute_policy_hash();
    packet.stable_hash = packet.compute_hash();
    assert!(packet
        .validate()
        .unwrap_err()
        .to_string()
        .contains("negative memory"));
}

#[test]
fn replay_receipt_derives_status_from_gates() {
    let gates = SuperReasoningGateResults {
        proof_gate: SuperReasoningGateReceipt::passed(vec!["proof".into()]),
        replay_gate: SuperReasoningGateReceipt::passed(vec!["replay".into()]),
        parity_gate: SuperReasoningGateReceipt::not_applicable(
            "hero judge run has no parity target",
            vec![],
        ),
        leak_gate: SuperReasoningGateReceipt::passed(vec!["leak".into()]),
        jankurai_gate: SuperReasoningGateReceipt::passed(vec!["audit".into()]),
    };
    let receipt = ReplayReceipt::from_gate_results(
        "run-1",
        "packet".into(),
        "policy".into(),
        "source".into(),
        vec![SuperReasoningArtifactReceipt {
            path: "artifact.json".into(),
            sha256: "abc".into(),
        }],
        gates,
    );
    assert_eq!(receipt.status, "passed");
    assert!(receipt.allows_completion());
    assert_eq!(receipt.artifact_hashes.len(), 1);
}

#[test]
fn replay_receipt_artifact_integrity_detects_tamper() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("artifact.json");
    std::fs::write(&artifact, b"{\"ok\":true}").unwrap();
    let sha = sha256_hex(b"{\"ok\":true}");
    let gates = SuperReasoningGateResults {
        proof_gate: SuperReasoningGateReceipt::passed(vec![]),
        replay_gate: SuperReasoningGateReceipt::passed(vec![]),
        parity_gate: SuperReasoningGateReceipt::passed(vec![]),
        leak_gate: SuperReasoningGateReceipt::passed(vec![]),
        jankurai_gate: SuperReasoningGateReceipt::passed(vec![]),
    };
    let receipt = ReplayReceipt::from_gate_results(
        "run-1",
        "packet".into(),
        "policy".into(),
        "source".into(),
        vec![SuperReasoningArtifactReceipt {
            path: artifact.display().to_string(),
            sha256: sha,
        }],
        gates,
    );
    receipt.verify_artifact_integrity().unwrap();
    std::fs::write(&artifact, b"{\"ok\":false}").unwrap();
    let err = receipt.verify_artifact_integrity().unwrap_err().to_string();
    assert!(err.contains("artifact hash mismatch"));
}

#[test]
fn replay_receipt_artifact_integrity_detects_missing_artifact() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let gates = SuperReasoningGateResults {
        proof_gate: SuperReasoningGateReceipt::passed(vec![]),
        replay_gate: SuperReasoningGateReceipt::passed(vec![]),
        parity_gate: SuperReasoningGateReceipt::passed(vec![]),
        leak_gate: SuperReasoningGateReceipt::passed(vec![]),
        jankurai_gate: SuperReasoningGateReceipt::passed(vec![]),
    };
    let receipt = ReplayReceipt::from_gate_results(
        "run-1",
        "packet".into(),
        "policy".into(),
        "source".into(),
        vec![SuperReasoningArtifactReceipt {
            path: missing.display().to_string(),
            sha256: "abc".into(),
        }],
        gates,
    );
    let err = receipt.verify_artifact_integrity().unwrap_err().to_string();
    assert!(err.contains("read receipted artifact"));
}

#[test]
fn packet_reconstructs_from_artifact_on_disk() {
    let dir = tempdir().unwrap();
    let packet = SuperReasoningPacket::hero_judge(
        "run-1",
        "objective",
        &SuperReasoningConfig::default(),
        dir.path(),
        "source-hash".to_string(),
        SuperReasoningBudgetContract {
            effective_generations: 1,
            model_call_budget: 8,
            search_query_budget: 1,
            search_page_budget: 2,
            max_parallel: 2,
            max_workers: 2,
        },
        ModelPolicy::default(),
    );
    let path = dir.path().join("packet.json");
    std::fs::write(&path, serde_json::to_string_pretty(&packet).unwrap()).unwrap();
    let reconstructed = SuperReasoningPacket::reconstruct_from_artifact(&path).unwrap();
    assert_eq!(reconstructed.stable_hash, packet.stable_hash);
    assert_eq!(reconstructed.policy_hash, packet.policy_hash);
}

#[test]
fn packet_reconstruction_rejects_tampered_policy_hash() {
    let dir = tempdir().unwrap();
    let packet = SuperReasoningPacket::hero_judge(
        "run-1",
        "objective",
        &SuperReasoningConfig::default(),
        dir.path(),
        "source-hash".to_string(),
        SuperReasoningBudgetContract {
            effective_generations: 1,
            model_call_budget: 8,
            search_query_budget: 1,
            search_page_budget: 2,
            max_parallel: 2,
            max_workers: 2,
        },
        ModelPolicy::default(),
    );
    let mut tampered: serde_json::Value = serde_json::to_value(&packet).unwrap();
    tampered["objective"] = serde_json::Value::String("hijacked".into());
    let path = dir.path().join("packet.json");
    std::fs::write(&path, serde_json::to_string_pretty(&tampered).unwrap()).unwrap();
    let err = SuperReasoningPacket::reconstruct_from_artifact(&path)
        .unwrap_err()
        .to_string();
    assert!(err.contains("policy hash") || err.contains("stable hash"));
}

#[test]
fn draft_super_master_plan_emits_nine_to_twelve_stages() {
    let plan = draft_super_master_plan(&super_target());
    assert!((SUPER_STAGE_MIN..=SUPER_STAGE_MAX).contains(&plan.stages.len()));
    assert_eq!(plan.tasks.len(), plan.stages.len() * 2);
    // First stage matches the canonical kickoff phase.
    assert!(plan.stages[0].name.eq_ignore_ascii_case("Source of truth"));
    // Drafted plan must pass the macro validator.
    validate_super_macro_plan(&plan).unwrap();

    // The 12-stage upper bound is reachable when configured.
    let cfg = SuperReasoningConfig {
        macro_stage_target: SUPER_STAGE_MAX,
        ..SuperReasoningConfig::default()
    };
    let max_plan = draft_super_master_plan_with_config(&super_target(), &cfg);
    assert_eq!(max_plan.stages.len(), SUPER_STAGE_MAX);
    validate_super_macro_plan(&max_plan).unwrap();
}

#[test]
fn validate_super_macro_plan_rejects_cycle() {
    let mut plan = draft_super_master_plan(&super_target());
    let first_id = plan.stages[0].id.clone();
    let last_idx = plan.stages.len() - 1;
    let last_id = plan.stages[last_idx].id.clone();
    // Introduce a back edge from first stage to last stage to force a cycle.
    plan.stages[0].dependencies.push(last_id);
    let err = validate_super_macro_plan(&plan).unwrap_err().to_string();
    assert!(
        err.contains("cycle"),
        "expected cycle error, got: {err} (first stage id={first_id})"
    );
}

#[test]
fn validate_super_macro_plan_rejects_duplicate_stage_id() {
    let mut plan = draft_super_master_plan(&super_target());
    let dup_id = plan.stages[0].id.clone();
    plan.stages[1].id = dup_id;
    let err = validate_super_macro_plan(&plan).unwrap_err().to_string();
    assert!(
        err.contains("duplicate"),
        "expected duplicate-id error, got: {err}"
    );
}

#[test]
fn parallel_phase_policy_default_caps_at_workspace_max() {
    let policy = ParallelPhasePolicy::default();
    assert!(policy.effective_max_parallel_phases() <= MAX_SUPERREASONING_WORKERS);
    assert!(policy.effective_per_phase_worker_cap() <= MAX_SUPERREASONING_WORKERS);

    // Overshoot is clamped down to the workspace cap.
    let oversized = ParallelPhasePolicy {
        max_parallel_phases: 9_999,
        per_phase_worker_cap: 9_999,
        ..ParallelPhasePolicy::default()
    };
    assert_eq!(
        oversized.effective_max_parallel_phases(),
        MAX_SUPERREASONING_WORKERS
    );
    assert_eq!(
        oversized.effective_per_phase_worker_cap(),
        MAX_SUPERREASONING_WORKERS
    );

    // Zero / underflow clamps up to 1.
    let zero = ParallelPhasePolicy {
        max_parallel_phases: 0,
        per_phase_worker_cap: 0,
        ..ParallelPhasePolicy::default()
    };
    assert_eq!(zero.effective_max_parallel_phases(), 1);
    assert_eq!(zero.effective_per_phase_worker_cap(), 1);
}
