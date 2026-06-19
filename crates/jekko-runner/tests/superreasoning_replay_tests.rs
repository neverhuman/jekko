//! Integration tests covering replay-receipt integrity, packet
//! reconstruction, and host-derived gate evidence on artifacts produced by a
//! deterministic Hero/Judge run. These complement the unit tests in
//! `superreasoning.rs` by exercising the full public runner path and then
//! independently re-verifying the persisted artifacts.

use std::fs;
use std::path::Path;

use jekko_runner::bootstrap_check;
use jekko_runner::hashing::sha256_hex;
use jekko_runner::hero_judge::HeroJudgeRunbook;
use jekko_runner::hero_judge_runner::run_hero_judge_run_with_db;
use jekko_runner::model_client::FakeModelClient;
use jekko_runner::superreasoning::{ReplayReceipt, SuperReasoningPacket};
use jekko_store::db::Db;
use tempfile::tempdir;

fn bootstrap_repo(dir: &Path) {
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    for file in bootstrap_check::CANONICAL_FILES {
        let abs = dir.join(file.rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, "").unwrap();
    }
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(
        dir.join("docs/zyal-research-loops.md"),
        "OpenQG research loops require verified evidence and receipts.",
    )
    .unwrap();
    fs::create_dir_all(dir.join("tips/rolling")).unwrap();
    fs::write(
        dir.join("tips/rolling/tip1.txt"),
        "admit falsifiable theories",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-q", "-m", "seed"])
        .current_dir(dir)
        .status()
        .unwrap();
}

fn deterministic_runbook() -> HeroJudgeRunbook {
    serde_yaml::from_str(
        r#"
job:
  name: openqg-hero-judge
  objective: Evolve OpenQG prompts.
hero_judge:
  generations: 1
  population:
    hero_lanes: 2
    judge_lanes: 1
    verifier_lanes: 1
    literature_lanes: 1
    red_team_lanes: 1
    max_parallel: 2
  budgets:
    model_calls: 12
    search_queries: 1
    search_pages: 2
"#,
    )
    .unwrap()
}

async fn run_deterministic(
    run_id: &str,
) -> (
    tempfile::TempDir,
    jekko_runner::hero_judge::HeroJudgeRunSummary,
) {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let report = run_hero_judge_run_with_db(
        dir.path(),
        run_id,
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        deterministic_runbook(),
        Some(1),
        false,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();
    (dir, report)
}

#[tokio::test]
async fn replay_receipt_artifact_hashes_match_persisted_files() {
    let (_dir, report) = run_deterministic("hero-judge-integrity").await;
    let receipt: ReplayReceipt =
        serde_json::from_slice(&fs::read(&report.replay_receipt_json).unwrap()).unwrap();
    assert_eq!(receipt.status, "passed");
    assert!(receipt.allows_completion());
    assert!(
        !receipt.artifact_hashes.is_empty(),
        "replay receipt must record at least one artifact hash"
    );
    // Verifying the receipt against its own artifacts must succeed.
    receipt.verify_artifact_integrity().unwrap();
    // And every recorded hash must match a fresh sha256 read of the path.
    for entry in &receipt.artifact_hashes {
        let bytes = fs::read(&entry.path)
            .unwrap_or_else(|_| panic!("receipted artifact must exist: {}", entry.path));
        assert_eq!(
            sha256_hex(&bytes),
            entry.sha256,
            "fresh hash for {} did not match receipt",
            entry.path
        );
    }
}

#[tokio::test]
async fn tampering_with_artifact_breaks_receipt_integrity() {
    let (_dir, report) = run_deterministic("hero-judge-tamper").await;
    let receipt: ReplayReceipt =
        serde_json::from_slice(&fs::read(&report.replay_receipt_json).unwrap()).unwrap();
    // Pick a non-empty artifact and corrupt it.
    let target = receipt
        .artifact_hashes
        .iter()
        .find(|entry| {
            fs::metadata(&entry.path)
                .map(|meta| meta.len() != 0)
                .unwrap_or(false)
        })
        .expect("at least one non-empty receipted artifact");
    let original = fs::read(&target.path).unwrap();
    let mut corrupted = original.clone();
    corrupted.extend_from_slice(b"\n# tampered\n");
    fs::write(&target.path, &corrupted).unwrap();
    let err = receipt.verify_artifact_integrity().unwrap_err().to_string();
    assert!(
        err.contains("artifact hash mismatch") && err.contains(target.path.as_str()),
        "tamper-detection error message should name the path: {err}"
    );
    // Restore so any cleanup in other tests stays predictable.
    fs::write(&target.path, &original).unwrap();
}

#[tokio::test]
async fn packet_reconstruction_matches_recorded_hash() {
    let (_dir, report) = run_deterministic("hero-judge-reconstruct").await;
    let reconstructed =
        SuperReasoningPacket::reconstruct_from_artifact(&report.superreasoning_packet_json)
            .unwrap();
    assert_eq!(
        reconstructed.stable_hash,
        report.superreasoning_packet_sha256
    );
    assert_eq!(reconstructed.stable_hash, reconstructed.compute_hash());
    assert_eq!(
        reconstructed.policy_hash,
        reconstructed.compute_policy_hash()
    );
}

#[tokio::test]
async fn forbidden_content_in_artifact_would_fail_leak_gate() {
    // Sanity check: the packet's forbidden_content list is non-trivial and
    // forbids storing raw chain-of-thought style strings. This ensures the
    // leak gate has teeth even when no future-state run accidentally writes
    // such content.
    let (_dir, report) = run_deterministic("hero-judge-leak-sanity").await;
    let packet: SuperReasoningPacket =
        serde_json::from_slice(&fs::read(&report.superreasoning_packet_json).unwrap()).unwrap();
    assert!(packet
        .artifact_contract
        .forbidden_content
        .iter()
        .any(|term| term.contains("raw_chain_of_thought")));
    assert!(packet
        .artifact_contract
        .forbidden_content
        .iter()
        .any(|term| term.contains("fixture_target_values_in_model_visible_artifacts")));
}

#[tokio::test]
async fn reviewer_packet_embeds_full_superreasoning_packet() {
    let (_dir, report) = run_deterministic("hero-judge-reviewer-embed").await;
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&report.reviewer_packet_json).unwrap()).unwrap();
    let embedded = value
        .get("superreasoning_packet")
        .expect("reviewer packet must embed superreasoning_packet per spec v2");
    assert_eq!(
        embedded["schema_version"], "zyal.superreasoning.packet.v1",
        "embedded packet must declare its schema_version"
    );
    let embedded_hash = embedded["stable_hash"]
        .as_str()
        .expect("embedded packet must carry stable_hash");
    assert_eq!(
        embedded_hash, report.superreasoning_packet_sha256,
        "embedded packet hash must match the run summary's recorded hash"
    );
    // Round-trip back through the typed packet to confirm validate() passes.
    let parsed: SuperReasoningPacket = serde_json::from_value(embedded.clone()).unwrap();
    parsed.validate().unwrap();
}

#[tokio::test]
async fn negative_memory_is_derived_from_real_scoreboard_rejections() {
    let (_dir, report) = run_deterministic("hero-judge-negative-memory").await;
    let text = fs::read_to_string(&report.negative_memory_jsonl).unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        !rows.is_empty(),
        "negative memory must contain at least the policy-invariant tail row"
    );
    // Every row must carry the canonical schema and reference the run.
    for row in &rows {
        assert_eq!(
            row["schema_version"], "zyal.superreasoning.negative_memory.v1",
            "every negative memory row must declare its schema"
        );
        assert_eq!(row["run_id"], report.run_id);
        assert!(row["id"].is_string());
        assert!(row["kind"].is_string());
    }
    // The policy-invariant row must be present so the ledger is never empty.
    let policy_row = rows
        .iter()
        .find(|row| row["kind"] == "policy_invariant")
        .expect("policy_invariant tail row must be present");
    assert_eq!(policy_row["status"], "verified");
}

#[tokio::test]
async fn replay_receipt_records_every_required_gate_explicitly() {
    let (_dir, report) = run_deterministic("hero-judge-gates").await;
    let receipt: ReplayReceipt =
        serde_json::from_slice(&fs::read(&report.replay_receipt_json).unwrap()).unwrap();
    // Each gate must have a status of "passed" or "not_applicable"; "pending"
    // or "failed" must never reach a completed run.
    for (name, gate) in [
        ("proof_gate", &receipt.gate_results.proof_gate),
        ("replay_gate", &receipt.gate_results.replay_gate),
        ("parity_gate", &receipt.gate_results.parity_gate),
        ("leak_gate", &receipt.gate_results.leak_gate),
        ("jankurai_gate", &receipt.gate_results.jankurai_gate),
    ] {
        assert!(
            gate.status == "passed" || gate.status == "not_applicable",
            "{name} should be passed or not_applicable at run completion, got {}: {:?}",
            gate.status,
            gate.message
        );
        assert!(
            !gate.evidence.is_empty() || gate.message.is_some() || gate.status == "not_applicable",
            "{name} should record evidence or an explicit message"
        );
    }
}
