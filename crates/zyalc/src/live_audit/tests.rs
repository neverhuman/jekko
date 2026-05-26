use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::*;

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn write_jsonl(path: &Path, values: &[Value]) {
    let mut body = String::new();
    for value in values {
        body.push_str(&serde_json::to_string(value).unwrap());
        body.push('\n');
    }
    fs::write(path, body).unwrap();
}

fn artifact(dir: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    json!({
        "path": path.display().to_string(),
        "sha256": sha256_hex(bytes),
    })
}

fn good_model_receipt() -> Value {
    json!({
        "schema_version": "zyal.model_receipt.v1",
        "id": "model-live-1",
        "run_id": "live-run-1",
        "kind": "literature_synthesis",
        "provider": "openrouter",
        "model": "openrouter/test-model",
        "status": "success",
        "success": true,
        "latency_ms": 42,
        "cost_usd": null,
        "response_sha256": "0123456789abcdef",
        "response_bytes": 128,
        "error": null,
        "budget_used": 1,
        "budget_remaining": 7,
        "route": "literature_synthesis",
        "credential_policy": "users-only",
        "credential_user_id": "user_1",
        "retry_count": 0,
    })
}

fn good_model_outcome() -> Value {
    json!({
        "ts": 1,
        "kind": "model_outcome",
        "run_id": "live-run-1",
        "data": {
            "kind": "literature_synthesis",
            "provider": "openrouter",
            "model": "openrouter/test-model",
            "success": true,
            "attempt": 1,
            "state": "parsed",
            "latency_ms": 42,
            "response_bytes": 128,
            "credential_policy": "users-only",
            "credential_user_id": "user_1",
            "retry_count": 0,
            "budget_used": 1,
            "budget_remaining": 7
        }
    })
}

fn write_good_run() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    let model_receipt = good_model_receipt();
    write_jsonl(&dir.join("model_receipts.jsonl"), &[model_receipt]);
    let model_receipts_bytes = fs::read(dir.join("model_receipts.jsonl")).unwrap();
    let claim = b"{\"schema_version\":\"zyal.superreasoning.claim.v1\"}\n";
    let unsupported = b"{\"schema_version\":\"zyal.superreasoning.unsupported_claim.v1\"}\n";
    let negative = b"{\"schema_version\":\"zyal.superreasoning.negative_memory.v1\"}\n";

    let claim_artifact = artifact(dir, "claim_ledger.jsonl", claim);
    let unsupported_artifact = artifact(dir, "unsupported_claims.jsonl", unsupported);
    let negative_artifact = artifact(dir, "negative_memory.jsonl", negative);
    let model_artifact = json!({
        "path": dir.join("model_receipts.jsonl").display().to_string(),
        "sha256": sha256_hex(&model_receipts_bytes),
    });

    let packet = json!({
        "schema_version": "zyal.superreasoning.packet.v1",
        "run_id": "live-run-1",
        "objective": "prove live audit",
        "source_runbook_sha256": "src",
        "effective_generations": 1,
        "budget_contract": {
            "effective_generations": 1,
            "model_call_budget": 8,
            "search_query_budget": 1,
            "search_page_budget": 2,
            "max_parallel": 1,
            "max_workers": 1
        },
        "model_route_contract": {},
        "lane_plan": [{
            "id": "literature",
            "role": "research",
            "route": "routine",
            "max_workers": 1,
            "required_artifacts": []
        }],
        "artifact_contract": {
            "required_artifacts": [
                "superreasoning_packet.json",
                "reviewer_packet.json",
                "replay_receipt.json",
                "model_receipts.jsonl",
                "claim_ledger.jsonl",
                "unsupported_claims.jsonl",
                "negative_memory.jsonl"
            ],
            "forbidden_content": ["raw_chain_of_thought", "process_env_credentials"],
            "claim_ledger": "claim_ledger.jsonl",
            "unsupported_claims_ledger": "unsupported_claims.jsonl",
            "negative_memory": "negative_memory.jsonl"
        },
        "privacy_contract": {
            "store_raw_reasoning": false,
            "users_only_credentials": true,
            "model_visible_target_values": false,
            "storage_safe_summaries_only": true
        },
        "promotion_gates": {
            "proof_gate": true,
            "replay_gate": true,
            "parity_gate": true,
            "leak_gate": true,
            "jankurai_gate": true
        },
        "credential_policy": "users-only",
        "policy_hash": "policy-hash",
        "replay_receipt": {},
        "stable_hash": "stable-hash",
    });
    write_json(&dir.join("superreasoning_packet.json"), &packet);

    let reviewer = json!({
        "run_id": "live-run-1",
        "superreasoning_packet": packet,
        "replay_receipt_path": dir.join("replay_receipt.json").display().to_string()
    });
    write_json(&dir.join("reviewer_packet.json"), &reviewer);

    let receipt = json!({
        "schema_version": "zyal.superreasoning.replay_receipt.v1",
        "run_id": "live-run-1",
        "packet_hash": "stable-hash",
        "policy_hash": "policy-hash",
        "source_runbook_sha256": "src",
        "artifact_hashes": [
            claim_artifact,
            unsupported_artifact,
            negative_artifact,
            model_artifact
        ],
        "gate_results": {
            "proof_gate": {"status": "passed", "required": true, "evidence": ["e"]},
            "replay_gate": {"status": "passed", "required": true, "evidence": ["e"]},
            "parity_gate": {"status": "not_applicable", "required": false, "evidence": [], "message": "no parity target"},
            "leak_gate": {"status": "passed", "required": true, "evidence": ["e"]},
            "jankurai_gate": {"status": "passed", "required": true, "evidence": ["e"]}
        },
        "replay_gate_passed": true,
        "parity_gate_passed": true,
        "leak_gate_passed": true,
        "jankurai_gate_passed": true,
        "proof_gate_passed": true,
        "status": "passed"
    });
    write_json(&dir.join("replay_receipt.json"), &receipt);

    let started = json!({
        "ts": 1,
        "kind": "run_started",
        "run_id": "live-run-1",
        "data": {
            "workflow": "zyal_hero_judge",
            "generations": 1,
            "live_model_calls": true,
            "credential_policy": "users-only",
            "mock_llm_set": false
        }
    });
    write_jsonl(&dir.join("events.jsonl"), &[started, good_model_outcome()]);
    temp
}

#[test]
fn strict_live_audit_passes_on_complete_fixture() {
    let temp = write_good_run();
    let report = audit(temp.path(), true).unwrap();
    assert_eq!(report.status, "passed", "failures: {:?}", report.failures);
    assert_eq!(report.model_receipt_count, 1);
    assert_eq!(report.model_outcome_event_count, 1);
}

#[test]
fn strict_live_audit_rejects_live_parse_substitution() {
    let temp = write_good_run();
    let mut events = load_jsonl_optional(&temp.path().join("events.jsonl"), &mut empty_report());
    events[1]["data"]["state"] = Value::String("live_parse_substitution".to_string());
    write_jsonl(&temp.path().join("events.jsonl"), &events);
    let report = audit(temp.path(), true).unwrap();
    assert_eq!(report.status, "failed");
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.contains("live_parse_substitution")));
}

#[test]
fn strict_live_audit_rejects_retryable_invalid_json_state() {
    let temp = write_good_run();
    let mut events = load_jsonl_optional(&temp.path().join("events.jsonl"), &mut empty_report());
    events[1]["data"]["state"] = Value::String("retryable_failure".to_string());
    write_jsonl(&temp.path().join("events.jsonl"), &events);
    let report = audit(temp.path(), true).unwrap();
    assert_eq!(report.status, "failed");
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.contains("expected parsed")));
}

#[test]
fn strict_live_audit_rejects_missing_credential_user_id() {
    let temp = write_good_run();
    write_jsonl(
        &temp.path().join("model_receipts.jsonl"),
        &[json!({
            "schema_version": "zyal.model_receipt.v1",
            "id": "model-live-1",
            "run_id": "live-run-1",
            "kind": "literature_synthesis",
            "provider": "openrouter",
            "model": "openrouter/test-model",
            "status": "success",
            "success": true,
            "latency_ms": 42,
            "response_sha256": "0123456789abcdef",
            "response_bytes": 128,
            "budget_used": 1,
            "budget_remaining": 7,
            "credential_policy": "users-only",
            "credential_user_id": null,
            "retry_count": 0
        })],
    );
    let report = audit(temp.path(), true).unwrap();
    assert_eq!(report.status, "failed");
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.contains("missing credential_user_id")));
}

#[test]
fn strict_live_audit_rejects_fake_provider() {
    let temp = write_good_run();
    write_jsonl(
        &temp.path().join("model_receipts.jsonl"),
        &[json!({
            "schema_version": "zyal.model_receipt.v1",
            "id": "model-live-1",
            "run_id": "live-run-1",
            "kind": "literature_synthesis",
            "provider": "fake",
            "model": "fake-model",
            "status": "success",
            "success": true,
            "latency_ms": 0,
            "response_sha256": "0123456789abcdef",
            "response_bytes": 128,
            "budget_used": 1,
            "budget_remaining": 7,
            "credential_policy": "users-only",
            "credential_user_id": "user_1",
            "retry_count": 0
        })],
    );
    let report = audit(temp.path(), true).unwrap();
    assert_eq!(report.status, "failed");
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.contains("provider fake")));
}

fn empty_report() -> LiveAuditReport {
    LiveAuditReport {
        schema_version: "zyal.live_audit.v1".to_string(),
        run_dir: PathBuf::new(),
        strict: true,
        replay_status: None,
        artifact_count: 0,
        model_receipt_count: 0,
        model_outcome_event_count: 0,
        status: "failed".to_string(),
        failures: Vec::new(),
        warnings: Vec::new(),
    }
}
