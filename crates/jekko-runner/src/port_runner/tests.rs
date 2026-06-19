use std::fs;
use std::path::Path;

use jekko_store::db::Db;
use tempfile::tempdir;

use super::*;
use crate::bootstrap_check;
use crate::model_client::FakeModelClient;
use crate::port::{PortRuntimeOptions, PortTargetRequest};
use crate::reasoning::AdvancedReasoningConfig;

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
    fs::create_dir_all(dir.join(".jankurai")).unwrap();
    fs::write(
        dir.join(".jankurai/repo-score.json"),
        r#"{"score": 95.0, "findings": [], "caps_applied": []}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn ping() {}\n").unwrap();
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

fn config() -> PortRunConfig {
    PortRunConfig {
        target: PortTargetRequest {
            target: "MiniKV".into(),
            replacement: "MiniKV Rust".into(),
            target_repo: None,
            replacement_repo: None,
            request: "port MiniKV".into(),
            worker_cap: 4,
        },
        fake_worker_cycle: true,
        allow_dirty: false,
        advanced_reasoning: AdvancedReasoningConfig::default(),
        runtime: PortRuntimeOptions::default(),
    }
}

#[tokio::test]
async fn fake_port_tick_persists_plan_events_and_worker_pass() {
    let dir = tempdir().unwrap();
    let db_dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open(db_dir.path().join("jekko.db")).unwrap();
    let report = run_port_tick_with_db(
        dir.path(),
        "run-port-1",
        config(),
        &FakeModelClient::success("plan"),
        &db,
    )
    .await
    .unwrap();

    assert_eq!(report.plan.stages.len(), 10);
    assert_eq!(report.fake_task_completed.as_deref(), Some("task-discover"));
    let event_path = dir.path().join("target/zyal/runs/run-port-1/events.jsonl");
    let events = fs::read_to_string(event_path).unwrap();
    assert!(events.contains("model_outcome"));
    assert!(events.contains("worker_pass"));
}

#[test]
fn reads_json_port_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("port.json");
    fs::write(
        &path,
        r#"{
          "target": "Reference",
          "replacement": "Candidate",
          "request": "port it",
          "worker_cap": 3
        }"#,
    )
    .unwrap();
    let config = read_port_run_config(&path).unwrap();
    assert_eq!(config.target.effective_worker_cap(), 3);
}
