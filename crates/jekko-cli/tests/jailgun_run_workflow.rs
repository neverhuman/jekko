//! Integration test for the example `jailgun-run` SuperWorkflow.
//!
//! Validates that the hand-authored flat manifest exercising the per-phase
//! `exec` seam (jailgun + ssh + agent) parses, validates, and plans into the
//! expected linear wave order via `jekko port-run --super --dry-run`. Live
//! execution of the jailgun/ssh phases is covered by the `walk` unit tests with
//! stub binaries; `--live` is gated off under CI so it is not exercised here.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<repo>/crates/jekko-cli`; pop two levels.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

fn manifest_path() -> PathBuf {
    repo_root().join("agent/superworkflows/jailgun-run.superworkflow.json")
}

#[test]
fn manifest_carries_the_exec_seam() {
    let path = manifest_path();
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let v: Value = serde_json::from_str(&text).expect("manifest is valid JSON");

    let phases = v["phases"].as_array().expect("phases array");
    assert_eq!(phases.len(), 9, "expected 9 phases");
    assert_eq!(v["memory"]["negative_memory"], true);

    let by_id = |id: &str| -> Value {
        phases
            .iter()
            .find(|p| p["id"] == id)
            .unwrap_or_else(|| panic!("missing phase {id}"))
            .clone()
    };

    // produce -> jailgun with N tabs and a prompt-free durable ref.
    let produce = by_id("produce");
    assert_eq!(produce["exec"]["kind"], "jailgun");
    assert_eq!(produce["exec"]["tabs"], 6);
    assert!(produce["exec"]["prompt_ref"].is_string());

    // ssh_deploy + verify -> ssh.
    assert_eq!(by_id("ssh_deploy")["exec"]["kind"], "ssh");
    assert_eq!(by_id("verify")["exec"]["kind"], "ssh");

    // research/reduce -> agent; review -> a distinct reviewer agent.
    assert_eq!(by_id("research")["exec"]["kind"], "agent");
    assert_eq!(by_id("review")["exec"]["name"], "code-reviewer");

    // frame has no exec (historical default-agent path).
    assert!(by_id("frame").get("exec").is_none());
}

#[test]
fn dry_run_plans_linear_waves() {
    let manifest = manifest_path();
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--dry-run")
        .output()
        .expect("port-run --dry-run");
    assert!(
        output.status.success(),
        "dry-run failed (manifest must parse + validate + plan): status={:?} stderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("dry-run output must be JSON: {e}\n{stdout}"));
    let waves = value["waves"].as_array().expect("waves array");

    // Linear DAG -> 9 single-phase waves, `frame` first, `signoff` last.
    assert_eq!(
        waves.len(),
        9,
        "expected 9 linear waves, got {}",
        waves.len()
    );
    let first: Vec<&str> = waves[0]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(first, vec!["frame"]);
    let last: Vec<&str> = waves
        .last()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(last, vec!["signoff"]);
    let total: usize = waves.iter().map(|w| w.as_array().unwrap().len()).sum();
    assert_eq!(total, 9, "every phase appears exactly once across waves");
}
