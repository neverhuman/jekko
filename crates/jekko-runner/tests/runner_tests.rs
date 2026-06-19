//! Integration tests for the tick-loop runner. Mirrors the unit tests that
//! previously lived in `runner.rs`; extracted here to keep that file under 300
//! LOC so the shape-score dimension stays above the floor.

use std::fs;
use std::process::Command;
use std::time::Duration;

use jekko_runner::bootstrap_check;
use jekko_runner::runner::{random_run_id, run_tick, RunnerConfig};
use tempfile::tempdir;

fn bootstrap_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
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
        dir.join("agent/audit-policy.toml"),
        r#"[decision]
min_score = 85
fail_on = ["critical", "high"]
advisory_on = ["medium", "low"]
"#,
    )
    .unwrap();
    fs::write(
        dir.join(".jankurai/repo-score.json"),
        r#"{"score": 95.0, "findings": [], "caps_applied": []}"#,
    )
    .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "seed"])
        .current_dir(dir)
        .status()
        .unwrap();
}

#[tokio::test]
async fn dry_run_tick_on_green_repo_emits_run_started_and_finished() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let config = RunnerConfig {
        repo: dir.path().to_path_buf(),
        run_id: "test-run".to_string(),
        pool_size: 2,
        integration_branch: None,
        allow_dirty: false,
        dry_run: true,
    };
    let report = run_tick(&config, 1).await.unwrap();
    assert!(report.classify.findings.is_empty());
    assert!(report.waves.is_empty());
    let events_path = dir.path().join("target/zyal/runs/test-run/events.jsonl");
    let text = fs::read_to_string(&events_path).unwrap();
    assert!(text.contains("run_started"));
    assert!(text.contains("run_finished"));
    assert!(dir.path().join("target/zyal/runner-events.jsonl").exists());
}

#[tokio::test]
async fn dry_run_tick_schedules_findings_into_waves() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    fs::write(
        dir.path().join(".jankurai/repo-score.json"),
        r#"{
            "score": 60.0,
            "findings": [
                {"rule_id": "HLT-001", "fingerprint": "fp1", "severity": "low", "path": "src/a.rs"},
                {"rule_id": "HLT-002", "fingerprint": "fp2", "severity": "low", "path": "src/b.rs"}
            ],
            "caps_applied": []
        }"#,
    )
    .unwrap();
    let config = RunnerConfig {
        repo: dir.path().to_path_buf(),
        run_id: "test-run-2".to_string(),
        pool_size: 2,
        integration_branch: None,
        allow_dirty: true,
        dry_run: true,
    };
    let report = run_tick(&config, 1).await.unwrap();
    assert_eq!(report.classify.findings.len(), 2);
    assert_eq!(report.waves.len(), 1);
    let receipts_path = dir.path().join("agent/zyal/receipts.sqlite");
    assert!(receipts_path.exists());
}

#[tokio::test]
async fn dirty_tree_aborts_unless_allow_dirty() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "junk").unwrap();
    let config = RunnerConfig {
        repo: dir.path().to_path_buf(),
        run_id: "test-run-3".to_string(),
        pool_size: 2,
        integration_branch: None,
        allow_dirty: false,
        dry_run: true,
    };
    let err = run_tick(&config, 1).await.unwrap_err();
    assert!(err.to_string().contains("dirty"));
}

#[tokio::test]
async fn allow_dirty_proceeds_through_tick() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    fs::write(dir.path().join("untracked.txt"), "junk").unwrap();
    let config = RunnerConfig {
        repo: dir.path().to_path_buf(),
        run_id: "test-run-4".to_string(),
        pool_size: 2,
        integration_branch: None,
        allow_dirty: true,
        dry_run: true,
    };
    let report = run_tick(&config, 1).await.unwrap();
    assert!(report.classify.findings.is_empty());
}

#[test]
fn random_run_id_is_lex_sortable_within_a_second() {
    let a = random_run_id();
    std::thread::sleep(Duration::from_millis(5));
    let b = random_run_id();
    let a_secs = &a[..8];
    let b_secs = &b[..8];
    assert!(a_secs.le(b_secs));
}
